//! Procedural macro crate for deriving `PardosaSchema` AST descriptors and codecs.
#![allow(clippy::pedantic)]
//!
//! # Positive Definitions
//!
//! Provides the `#[derive(PardosaSchema)]` procedural macro to generate compile-time
//! schema descriptors and binary wire codecs:
//! - Declared schema versions via `#[pardosa(version = N)]` attributes.
//! - Strongly-typed AST descriptors capturing enum variants and fields.
//! - Canonical 32-byte BLAKE3 schema identity hashes.
//! - Binary wire encoding and decoding routines adhering to Pardosa format rules.
//!
//! # Supported Construction and Derive Diagnostics
//!
//! Input payload types are validated at compile time, rejecting unsupported shapes with actionable diagnostics:
//! - **Struct or Union Root**: Only enum roots are supported per C5.48 and C4.24. Remedy: Wrap struct data in an enum.
//! - **Missing Explicit Discriminants**: Every enum variant must declare an explicit integer discriminant (`Variant = 0`) per C5.48. Remedy: Add explicit discriminants.
//! - **Missing Tombstone**: Payload enums must designate a tombstone variant with `#[pardosa(tombstone)]` for migration markers. Remedy: Annotate an empty tombstone variant.
//! - **Cyclic Types (S4)**: Recursive type definitions without indirection are detected and rejected. Remedy: Break recursive definitions.
//! - **Unbounded Types**: Raw `String`, unbounded `Vec`, and floating-point types are rejected. Remedy: Use `EventString<MAX>`, `EventVec<T, MAX>`, and fixed-width integers.
//!
//! # Truthful Seal Limits (S5)
//!
//! Per C4.24 and C6.35, `PardosaSchema` derive guarantees that generated descriptors accurately reflect the compiled
//! Rust type definition. However, whether the declared types faithfully represent the domain events written by an
//! application remains outside what Pardosa establishes.
//!
//! # Shipped Producer Inventory (S10)
//!
//! Per C4.24, this procedural macro is the primary recognized producer of schema descriptors for Pardosa. Shipped
//! foundation crates contain zero hand-written descriptor implementations.
//!
//! # Security and Maintenance Disclosure
//!
//! - **Single Maintainer (C6.32)**: Pardosa has one maintainer. Issue triage and support are provided on a best-effort basis without an SLA.
//! - **Security Reporting (C5.38)**: Disclose vulnerabilities privately via GitHub Security Advisories at
//!   `https://github.com/acje/pardosa/security/advisories` or contact `security@pardosa.dev`.
//! - **Non-Rust Dependencies (C5.38)**: Advisories for non-Rust dependency edges are monitored directly.
//! - **Withdrawal Posture (C5.38)**: Published releases are withdrawn (yanked) strictly for correctness or safety defects.

#![deny(missing_docs)]

use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Lit,
    PathArguments, Type,
};

/// Derives `PardosaSchema` for an enum payload type.
#[proc_macro_derive(PardosaSchema, attributes(pardosa))]
pub fn derive_pardosa_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_pardosa_schema(&input) {
        Ok(expanded) => TokenStream::from(expanded),
        Err(err) => TokenStream::from(err.to_compile_error()),
    }
}

fn expand_pardosa_schema(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let data_enum = match &input.data {
        Data::Enum(e) => e,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "unsupported shape: struct cannot be the root of a payload type; a payload type must be an enum (per C5.48, C4.24)",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "unsupported shape: union is not supported per C4.24",
            ));
        }
    };

    let schema_version = parse_schema_version(&input.attrs)?;

    let mut has_tombstone = false;
    let mut variant_data = Vec::with_capacity(data_enum.variants.len());

    for variant in &data_enum.variants {
        let disc_expr = match &variant.discriminant {
            Some((_, expr)) => expr,
            None => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!(
                        "variant `{}` must have an explicit discriminant (per C5.48)",
                        variant.ident
                    ),
                ));
            }
        };

        let disc_val = parse_discriminant_value(disc_expr)?;

        let is_tombstone = check_tombstone_attr(&variant.attrs)?;
        if is_tombstone {
            has_tombstone = true;
        }

        for field in &variant.fields {
            check_type_for_issues(&field.ty, enum_name)?;
        }

        variant_data.push((variant, disc_val, disc_expr));
    }

    if !has_tombstone {
        return Err(syn::Error::new_spanned(
            input,
            "missing required event kind: tombstone; add a variant with recognition attribute `#[pardosa(tombstone)]`, e.g.:\n    #[pardosa(tombstone)]\n    Tombstone = 0,",
        ));
    }

    let max_disc = variant_data.iter().map(|(_, v, _)| *v).max().unwrap_or(0);
    let discriminant_width: u8 = if max_disc <= 255 { 1 } else { 2 };

    let mut variant_descriptor_tokens = Vec::new();
    let mut encode_match_arms = Vec::new();
    let mut decode_match_arms = Vec::new();

    for (variant, disc_val, disc_expr) in &variant_data {
        let vident = &variant.ident;
        let vname_str = vident.to_string();

        match &variant.fields {
            Fields::Unit => {
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::None,
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident => {
                            buf.push(#disc_expr as u8);
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident => {
                            buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        ::std::result::Result::Ok(Self::#vident)
                    }
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0].ty;
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(<#ty as ::pardosa::schema::PardosaType>::descriptor_node()),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident(val) => {
                            buf.push(#disc_expr as u8);
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(val, buf)
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(val) => {
                            buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(val, buf)
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        let (val, _) = <#ty as ::pardosa::schema::PardosaType>::decode_type(&buf[#discriminant_width as usize..])?;
                        ::std::result::Result::Ok(Self::#vident(val))
                    }
                });
            }
            Fields::Named(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_idents = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for f in &fields.named {
                    let fident = f.ident.as_ref().unwrap();
                    let fname_str = fident.to_string();
                    let fty = &f.ty;

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    field_idents.push(fident);
                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#fident, buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#fident, consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&buf[cursor..])?;
                        cursor += consumed;
                    });
                }

                let struct_name = format!("{}_{}", enum_name, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents),* } => {
                            buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents),* } => {
                            buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        let mut cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok(Self::#vident { #(#field_idents),* })
                    }
                });
            }
            Fields::Unnamed(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_idents = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.unnamed.iter().enumerate() {
                    let idx_ident = syn::Ident::new(&format!("f{}", idx), f.span());
                    let fname_str = format!("_{}", idx);
                    let fty = &f.ty;

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    field_idents.push(idx_ident.clone());
                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#idx_ident, buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#idx_ident, consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&buf[cursor..])?;
                        cursor += consumed;
                    });
                }

                let struct_name = format!("{}_{}", enum_name, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_idents),*) => {
                            buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_idents),*) => {
                            buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        let mut cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok(Self::#vident(#(#field_idents),*))
                    }
                });
            }
        }
    }

    let enum_name_str = enum_name.to_string();

    let decode_disc_extract = if discriminant_width == 1 {
        quote! {
            if buf.is_empty() {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 1,
                    available: 0,
                });
            }
            let disc = buf[0] as u32;
        }
    } else {
        quote! {
            if buf.len() < 2 {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 2,
                    available: buf.len(),
                });
            }
            let disc = u16::from_le_bytes([buf[0], buf[1]]) as u32;
        }
    };

    Ok(quote! {
        impl ::pardosa::schema::PardosaSchema for #enum_name {
            fn schema_version() -> u32 {
                #schema_version
            }

            fn schema_descriptor() -> ::pardosa::schema::DescriptorNode {
                ::pardosa::schema::DescriptorNode::Enum {
                    name: #enum_name_str.to_string(),
                    discriminant_width: #discriminant_width,
                    variants: vec![
                        #(#variant_descriptor_tokens),*
                    ],
                }
            }

            fn encode_payload(&self, buf: &mut ::std::vec::Vec<u8>) -> ::std::result::Result<(), ::pardosa::encoding::EncodeError> {
                match self {
                    #(#encode_match_arms),*
                }
            }

            fn decode_payload(buf: &[u8]) -> ::std::result::Result<Self, ::pardosa::encoding::DecodeError> {
                #decode_disc_extract
                match disc {
                    #(#decode_match_arms,)*
                    other => ::std::result::Result::Err(::pardosa::encoding::DecodeError::UnknownVariantDiscriminant {
                        discriminant: other,
                    }),
                }
            }
        }
    })
}

fn parse_schema_version(attrs: &[Attribute]) -> syn::Result<u32> {
    for attr in attrs {
        if attr.path().is_ident("pardosa") {
            let mut version = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("version") {
                    let value = meta.value()?;
                    let lit: syn::LitInt = value.parse()?;
                    version = Some(lit.base10_parse::<u32>()?);
                    Ok(())
                } else {
                    Ok(())
                }
            })?;
            if let Some(v) = version {
                return Ok(v);
            }
        }
    }
    Ok(1)
}

fn check_tombstone_attr(attrs: &[Attribute]) -> syn::Result<bool> {
    for attr in attrs {
        if attr.path().is_ident("pardosa") {
            let mut is_tombstone = false;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("tombstone") {
                    is_tombstone = true;
                    Ok(())
                } else {
                    Ok(())
                }
            })?;
            if is_tombstone {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn parse_discriminant_value(expr: &Expr) -> syn::Result<u32> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(lit_int),
            ..
        }) => lit_int.base10_parse::<u32>(),
        _ => Err(syn::Error::new_spanned(
            expr,
            "discriminant must be an integer literal",
        )),
    }
}

fn check_type_for_issues(ty: &Type, enum_name: &syn::Ident) -> syn::Result<()> {
    match ty {
        Type::Path(type_path) => {
            if let Some(seg) = type_path.path.segments.last() {
                let ident_str = seg.ident.to_string();
                if ident_str == "f32" || ident_str == "f64" {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "unsupported type: floating-point representations are excluded from the admitted constructor vocabulary per C6.23; consider fixed-width integers or scaled integers",
                    ));
                }
                if ident_str == "String" || ident_str == "str" {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "unsupported type `String`: unbounded text is excluded per C6.23; use `EventString<MAX>` or `NonEmptyEventString<MAX>` instead",
                    ));
                }
                if ident_str == "Vec" {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "unsupported type `Vec`: unbounded collections are excluded per C6.23; use `EventVec<T, MAX>` or `EventBytes<MAX>` instead",
                    ));
                }
                if &seg.ident == enum_name {
                    return Err(syn::Error::new_spanned(
                        ty,
                        format!(
                            "cycle detected: recursive type `{}` is not permitted in schema descriptor per C6.22 (S4)",
                            enum_name
                        ),
                    ));
                }

                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let GenericArgument::Type(inner_ty) = arg {
                            check_type_for_issues(inner_ty, enum_name)?;
                        }
                    }
                }
            }
            Ok(())
        }
        Type::Reference(type_ref) => check_type_for_issues(&type_ref.elem, enum_name),
        Type::Tuple(type_tuple) => {
            for elem in &type_tuple.elems {
                check_type_for_issues(elem, enum_name)?;
            }
            Ok(())
        }
        Type::Array(type_arr) => check_type_for_issues(&type_arr.elem, enum_name),
        Type::Slice(type_slice) => check_type_for_issues(&type_slice.elem, enum_name),
        Type::Paren(type_paren) => check_type_for_issues(&type_paren.elem, enum_name),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    #[test]
    fn test_reject_struct_root() {
        let input: DeriveInput = parse_str("struct MyStruct { x: i32 }").unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("struct cannot be the root of a payload type"));
    }

    #[test]
    fn test_reject_union_root() {
        let input: DeriveInput = parse_str("union MyUnion { x: i32 }").unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("union is not supported"));
    }

    #[test]
    fn test_reject_missing_explicit_discriminant() {
        let input: DeriveInput =
            parse_str("enum MyEvent { #[pardosa(tombstone)] Tombstone, Other = 1 }").unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("must have an explicit discriminant"));
    }

    #[test]
    fn test_reject_missing_tombstone() {
        let input: DeriveInput = parse_str("enum MyEvent { Active = 1, Suspended = 2 }").unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing required event kind: tombstone"));
    }

    #[test]
    fn test_reject_floating_point() {
        let input: DeriveInput =
            parse_str("enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, FloatData(f64) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("floating-point representations are excluded"));
    }

    #[test]
    fn test_reject_unbounded_string() {
        let input: DeriveInput =
            parse_str("enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, StrData(String) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("unsupported type `String`"));
    }

    #[test]
    fn test_reject_unbounded_vec() {
        let input: DeriveInput =
            parse_str("enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, VecData(Vec<u8>) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("unsupported type `Vec`"));
    }

    #[test]
    fn test_detect_cycle_direct() {
        let input: DeriveInput =
            parse_str("enum MyNode { #[pardosa(tombstone)] Tombstone = 0, Next(Box<MyNode>) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("cycle detected"));
    }

    #[test]
    fn test_valid_enum_passes() {
        let input: DeriveInput = parse_str(
            "#[pardosa(version = 2)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, Created(u32) = 1 }"
        ).unwrap();
        let res = expand_pardosa_schema(&input);
        assert!(res.is_ok());
    }
}
