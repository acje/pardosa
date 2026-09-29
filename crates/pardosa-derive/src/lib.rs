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

/// Derives `PardosaType` for a struct or enum admitted type.
#[proc_macro_derive(PardosaType, attributes(pardosa))]
pub fn derive_pardosa_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_pardosa_type(&input) {
        Ok(expanded) => TokenStream::from(expanded),
        Err(err) => TokenStream::from(err.to_compile_error()),
    }
}

fn expand_pardosa_type(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    match &input.data {
        Data::Struct(data_struct) => expand_pardosa_type_struct(input, data_struct),
        Data::Enum(data_enum) => expand_pardosa_type_enum(input, data_enum),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "unsupported shape: union is not supported per C4.24",
        )),
    }
}

fn expand_pardosa_type_struct(
    input: &DeriveInput,
    data_struct: &syn::DataStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let ident_str = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut field_types = Vec::new();

    let (desc, encode_body, decode_body) = match &data_struct.fields {
        Fields::Unit => (
            quote! {
                ::pardosa::schema::DescriptorNode::Struct {
                    name: #ident_str.to_string(),
                    fields: ::std::vec::Vec::new(),
                }
            },
            quote! {
                ::std::result::Result::Ok(())
            },
            quote! {
                ::std::result::Result::Ok((Self, 0))
            },
        ),
        Fields::Named(fields) => {
            if fields.named.is_empty() {
                (
                    quote! {
                        ::pardosa::schema::DescriptorNode::Struct {
                            name: #ident_str.to_string(),
                            fields: ::std::vec::Vec::new(),
                        }
                    },
                    quote! {
                        ::std::result::Result::Ok(())
                    },
                    quote! {
                        ::std::result::Result::Ok((Self {}, 0))
                    },
                )
            } else {
                let mut field_descriptors = Vec::new();
                let mut field_idents = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.named.iter().enumerate() {
                    let fident = f.ident.as_ref().expect("named field has ident");
                    let fname_str = fident.to_string();
                    let fty = &f.ty;
                    check_type_for_issues(fty, ident)?;
                    field_types.push(fty);

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    field_idents.push(fident);
                    field_vars.push(field_var.clone());

                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(&self.#fident, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                (
                    quote! {
                        ::pardosa::schema::DescriptorNode::Struct {
                            name: #ident_str.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }
                    },
                    quote! {
                        #(#field_encodes)*
                        ::std::result::Result::Ok(())
                    },
                    quote! {
                        let mut __pardosa_cursor = 0usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok((Self { #(#field_idents: #field_vars),* }, __pardosa_cursor))
                    },
                )
            }
        }
        Fields::Unnamed(fields) => {
            if fields.unnamed.is_empty() {
                (
                    quote! {
                        ::pardosa::schema::DescriptorNode::Struct {
                            name: #ident_str.to_string(),
                            fields: ::std::vec::Vec::new(),
                        }
                    },
                    quote! {
                        ::std::result::Result::Ok(())
                    },
                    quote! {
                        ::std::result::Result::Ok((Self(), 0))
                    },
                )
            } else {
                let mut field_descriptors = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.unnamed.iter().enumerate() {
                    let syn_idx = syn::Index::from(idx);
                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    let fname_str = format!("_{idx}");
                    let fty = &f.ty;
                    check_type_for_issues(fty, ident)?;
                    field_types.push(fty);

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    field_vars.push(field_var.clone());
                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(&self.#syn_idx, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                (
                    quote! {
                        ::pardosa::schema::DescriptorNode::Struct {
                            name: #ident_str.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }
                    },
                    quote! {
                        #(#field_encodes)*
                        ::std::result::Result::Ok(())
                    },
                    quote! {
                        let mut __pardosa_cursor = 0usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok((Self(#(#field_vars),*), __pardosa_cursor))
                    },
                )
            }
        }
    };

    let type_depth_tokens = if field_types.is_empty() {
        quote! { 0usize }
    } else {
        quote! {
            {
                let mut __pardosa_max_depth = 0usize;
                #(
                    let __pardosa_d = <#field_types as ::pardosa::schema::PardosaType>::TYPE_DEPTH + 1;
                    if __pardosa_d > __pardosa_max_depth {
                        __pardosa_max_depth = __pardosa_d;
                    }
                )*
                assert!(__pardosa_max_depth <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
                __pardosa_max_depth
            }
        }
    };

    let const_assert = if input.generics.params.is_empty() {
        quote! {
            const _: () = assert!(<#ident as ::pardosa::schema::PardosaType>::TYPE_DEPTH <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics ::pardosa::schema::PardosaType for #ident #ty_generics #where_clause {
            const TYPE_DEPTH: usize = #type_depth_tokens;

            fn descriptor_node() -> ::pardosa::schema::DescriptorNode {
                assert!(Self::TYPE_DEPTH <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
                #desc
            }

            fn encode_type(&self, __pardosa_buf: &mut ::std::vec::Vec<u8>) -> ::std::result::Result<(), ::pardosa::encoding::EncodeError> {
                #encode_body
            }

            fn decode_type(__pardosa_buf: &[u8]) -> ::std::result::Result<(Self, usize), ::pardosa::encoding::DecodeError> {
                #decode_body
            }
        }

        #const_assert
    })
}

fn expand_pardosa_type_enum(
    input: &DeriveInput,
    data_enum: &syn::DataEnum,
) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let enum_name_str = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    if data_enum.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            input,
            "enum must have at least one variant",
        ));
    }

    let mut variant_data = Vec::with_capacity(data_enum.variants.len());
    let mut seen_discriminants = std::collections::HashSet::new();

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
        if disc_val > 65535 {
            return Err(syn::Error::new_spanned(
                disc_expr,
                format!(
                    "variant `{}` discriminant {} exceeds maximum allowed value 65535 (per M4)",
                    variant.ident, disc_val
                ),
            ));
        }
        if !seen_discriminants.insert(disc_val) {
            return Err(syn::Error::new_spanned(
                disc_expr,
                format!(
                    "duplicate discriminant {} found on variant `{}` (per M4)",
                    disc_val, variant.ident
                ),
            ));
        }

        for field in &variant.fields {
            check_type_for_issues(&field.ty, ident)?;
        }

        variant_data.push((variant, disc_val, disc_expr));
    }

    let max_disc = variant_data.iter().map(|(_, v, _)| *v).max().unwrap_or(0);
    let discriminant_width: u8 = if max_disc <= 255 { 1 } else { 2 };

    let mut variant_descriptor_tokens = Vec::new();
    let mut encode_match_arms = Vec::new();
    let mut decode_match_arms = Vec::new();
    let mut variant_depth_exprs = Vec::new();

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
                            __pardosa_buf.push(#disc_expr as u8);
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => ::std::result::Result::Ok((Self::#vident, #discriminant_width as usize))
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0].ty;
                variant_depth_exprs.push(quote! {
                    <#ty as ::pardosa::schema::PardosaType>::TYPE_DEPTH + 1
                });

                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(<#ty as ::pardosa::schema::PardosaType>::descriptor_node()),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident(__pardosa_field_0) => {
                            __pardosa_buf.push(#disc_expr as u8);
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(__pardosa_field_0, __pardosa_buf)
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(__pardosa_field_0) => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(__pardosa_field_0, __pardosa_buf)
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let (__pardosa_field_0, __pardosa_consumed) = <#ty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[#discriminant_width as usize..])?;
                        let __pardosa_cursor = ::pardosa::encoding::checked_advance(#discriminant_width as usize, __pardosa_consumed, __pardosa_buf.len())?;
                        ::std::result::Result::Ok((Self::#vident(__pardosa_field_0), __pardosa_cursor))
                    }
                });
            }
            Fields::Named(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_idents = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.named.iter().enumerate() {
                    let fident = f.ident.as_ref().expect("named field has ident");
                    let fname_str = fident.to_string();
                    let fty = &f.ty;
                    variant_depth_exprs.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::TYPE_DEPTH + 2
                    });

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    field_idents.push(fident);
                    field_vars.push(field_var.clone());

                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#field_var, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                let struct_name = format!("{}_{}", ident, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents: #field_vars),* } => {
                            __pardosa_buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents: #field_vars),* } => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let mut __pardosa_cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok((Self::#vident { #(#field_idents: #field_vars),* }, __pardosa_cursor))
                    }
                });
            }
            Fields::Unnamed(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.unnamed.iter().enumerate() {
                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    let fname_str = format!("_{idx}");
                    let fty = &f.ty;
                    variant_depth_exprs.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::TYPE_DEPTH + 2
                    });

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    field_vars.push(field_var.clone());
                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#field_var, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                let struct_name = format!("{}_{}", ident, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_vars),*) => {
                            __pardosa_buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_vars),*) => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let mut __pardosa_cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        ::std::result::Result::Ok((Self::#vident(#(#field_vars),*), __pardosa_cursor))
                    }
                });
            }
        }
    }

    let decode_disc_extract = if discriminant_width == 1 {
        quote! {
            if __pardosa_buf.is_empty() {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 1,
                    available: 0,
                });
            }
            let disc = __pardosa_buf[0] as u32;
        }
    } else {
        quote! {
            if __pardosa_buf.len() < 2 {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 2,
                    available: __pardosa_buf.len(),
                });
            }
            let disc = u16::from_le_bytes([__pardosa_buf[0], __pardosa_buf[1]]) as u32;
        }
    };

    let type_depth_tokens = if variant_depth_exprs.is_empty() {
        quote! { 0usize }
    } else {
        quote! {
            {
                let mut __pardosa_max_depth = 0usize;
                #(
                    let __pardosa_d = #variant_depth_exprs;
                    if __pardosa_d > __pardosa_max_depth {
                        __pardosa_max_depth = __pardosa_d;
                    }
                )*
                assert!(__pardosa_max_depth <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
                __pardosa_max_depth
            }
        }
    };

    let const_assert = if input.generics.params.is_empty() {
        quote! {
            const _: () = assert!(<#ident as ::pardosa::schema::PardosaType>::TYPE_DEPTH <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics ::pardosa::schema::PardosaType for #ident #ty_generics #where_clause {
            const TYPE_DEPTH: usize = #type_depth_tokens;

            fn descriptor_node() -> ::pardosa::schema::DescriptorNode {
                assert!(Self::TYPE_DEPTH <= ::pardosa::schema::MAX_DESCRIPTOR_DEPTH);
                ::pardosa::schema::DescriptorNode::Enum {
                    name: #enum_name_str.to_string(),
                    discriminant_width: #discriminant_width,
                    variants: ::std::vec![
                        #(#variant_descriptor_tokens),*
                    ],
                }
            }

            fn encode_type(&self, __pardosa_buf: &mut ::std::vec::Vec<u8>) -> ::std::result::Result<(), ::pardosa::encoding::EncodeError> {
                match self {
                    #(#encode_match_arms),*
                }
            }

            fn decode_type(__pardosa_buf: &[u8]) -> ::std::result::Result<(Self, usize), ::pardosa::encoding::DecodeError> {
                #decode_disc_extract
                match disc {
                    #(#decode_match_arms,)*
                    other => ::std::result::Result::Err(::pardosa::encoding::DecodeError::UnknownVariantDiscriminant {
                        discriminant: other,
                    }),
                }
            }
        }

        #const_assert
    })
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

    let schema_version = parse_schema_version(input)?;
    let depth_assertions = data_enum.variants.iter().flat_map(|variant| {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "syn AST expression/type variants"
        )]
        let overhead = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => 1usize,
            _ => 2usize,
        };
        variant.fields.iter().map(move |field| {
            let ty = &field.ty;
            quote! {
                const _: () = {
                    let allowed_depth = ::pardosa::schema::MAX_DESCRIPTOR_DEPTH - #overhead;
                    assert!(<#ty as ::pardosa::schema::PardosaType>::TYPE_DEPTH <= allowed_depth);
                };
            }
        })
    });

    let mut has_tombstone = false;
    let mut variant_data = Vec::with_capacity(data_enum.variants.len());
    let mut seen_discriminants = std::collections::HashSet::new();

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
        if disc_val > 65535 {
            return Err(syn::Error::new_spanned(
                disc_expr,
                format!(
                    "variant `{}` discriminant {} exceeds maximum allowed value 65535 (per M4)",
                    variant.ident, disc_val
                ),
            ));
        }
        if !seen_discriminants.insert(disc_val) {
            return Err(syn::Error::new_spanned(
                disc_expr,
                format!(
                    "duplicate discriminant {} found on variant `{}` (per M4)",
                    disc_val, variant.ident
                ),
            ));
        }

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
                            __pardosa_buf.push(#disc_expr as u8);
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        let consumed = #discriminant_width as usize;
                        if consumed != __pardosa_buf.len() {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: consumed,
                                available: __pardosa_buf.len(),
                            });
                        }
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
                        Self::#vident(__pardosa_field_0) => {
                            __pardosa_buf.push(#disc_expr as u8);
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(__pardosa_field_0, __pardosa_buf)
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(__pardosa_field_0) => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            <#ty as ::pardosa::schema::PardosaType>::encode_type(__pardosa_field_0, __pardosa_buf)
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let (__pardosa_field_0, __pardosa_consumed) = <#ty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[#discriminant_width as usize..])?;
                        let __pardosa_cursor = ::pardosa::encoding::checked_advance(#discriminant_width as usize, __pardosa_consumed, __pardosa_buf.len())?;
                        if __pardosa_cursor != __pardosa_buf.len() {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: __pardosa_cursor,
                                available: __pardosa_buf.len(),
                            });
                        }
                        ::std::result::Result::Ok(Self::#vident(__pardosa_field_0))
                    }
                });
            }
            Fields::Named(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_idents = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.named.iter().enumerate() {
                    let fident = f.ident.as_ref().expect("named field has ident");
                    let fname_str = fident.to_string();
                    let fty = &f.ty;

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    field_idents.push(fident);
                    field_vars.push(field_var.clone());

                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#field_var, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                let struct_name = format!("{}_{}", enum_name, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents: #field_vars),* } => {
                            __pardosa_buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident { #(#field_idents: #field_vars),* } => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let mut __pardosa_cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        if __pardosa_cursor != __pardosa_buf.len() {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: __pardosa_cursor,
                                available: __pardosa_buf.len(),
                            });
                        }
                        ::std::result::Result::Ok(Self::#vident { #(#field_idents: #field_vars),* })
                    }
                });
            }
            Fields::Unnamed(fields) => {
                let mut field_descriptors = Vec::new();
                let mut field_vars = Vec::new();
                let mut field_encodes = Vec::new();
                let mut field_decodes = Vec::new();

                for (idx, f) in fields.unnamed.iter().enumerate() {
                    let field_var = syn::Ident::new(&format!("__pardosa_field_{idx}"), f.span());
                    let fname_str = format!("_{idx}");
                    let fty = &f.ty;

                    field_descriptors.push(quote! {
                        ::pardosa::schema::FieldDescriptor {
                            name: #fname_str.to_string(),
                            node: <#fty as ::pardosa::schema::PardosaType>::descriptor_node(),
                        }
                    });

                    field_vars.push(field_var.clone());
                    field_encodes.push(quote! {
                        <#fty as ::pardosa::schema::PardosaType>::encode_type(#field_var, __pardosa_buf)?;
                    });

                    field_decodes.push(quote! {
                        let (#field_var, __pardosa_consumed) = <#fty as ::pardosa::schema::PardosaType>::decode_type(&__pardosa_buf[__pardosa_cursor..])?;
                        __pardosa_cursor = ::pardosa::encoding::checked_advance(__pardosa_cursor, __pardosa_consumed, __pardosa_buf.len())?;
                    });
                }

                let struct_name = format!("{}_{}", enum_name, vname_str);
                variant_descriptor_tokens.push(quote! {
                    ::pardosa::schema::VariantDescriptor {
                        discriminant: #disc_expr as u32,
                        name: #vname_str.to_string(),
                        payload: ::std::option::Option::Some(::pardosa::schema::DescriptorNode::Struct {
                            name: #struct_name.to_string(),
                            fields: ::std::vec![#(#field_descriptors),*],
                        }),
                    }
                });

                if discriminant_width == 1 {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_vars),*) => {
                            __pardosa_buf.push(#disc_expr as u8);
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                } else {
                    encode_match_arms.push(quote! {
                        Self::#vident(#(#field_vars),*) => {
                            __pardosa_buf.extend_from_slice(&(#disc_expr as u16).to_le_bytes());
                            #(#field_encodes)*
                            ::std::result::Result::Ok(())
                        }
                    });
                }

                decode_match_arms.push(quote! {
                    #disc_val => {
                        if __pardosa_buf.len() < #discriminant_width as usize {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: #discriminant_width as usize,
                                available: __pardosa_buf.len(),
                            });
                        }
                        let mut __pardosa_cursor = #discriminant_width as usize;
                        #(#field_decodes)*
                        if __pardosa_cursor != __pardosa_buf.len() {
                            return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                                expected: __pardosa_cursor,
                                available: __pardosa_buf.len(),
                            });
                        }
                        ::std::result::Result::Ok(Self::#vident(#(#field_vars),*))
                    }
                });
            }
        }
    }

    let enum_name_str = enum_name.to_string();

    let decode_disc_extract = if discriminant_width == 1 {
        quote! {
            if __pardosa_buf.is_empty() {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 1,
                    available: 0,
                });
            }
            let disc = __pardosa_buf[0] as u32;
        }
    } else {
        quote! {
            if __pardosa_buf.len() < 2 {
                return ::std::result::Result::Err(::pardosa::encoding::DecodeError::TruncatedPayload {
                    expected: 2,
                    available: __pardosa_buf.len(),
                });
            }
            let disc = u16::from_le_bytes([__pardosa_buf[0], __pardosa_buf[1]]) as u32;
        }
    };

    Ok(quote! {
        impl ::pardosa::schema::PardosaSchema for #enum_name {
            const SCHEMA_VERSION: u32 = #schema_version;

            fn schema_descriptor() -> ::pardosa::schema::DescriptorNode {
                ::pardosa::schema::DescriptorNode::Enum {
                    name: #enum_name_str.to_string(),
                    discriminant_width: #discriminant_width,
                    variants: ::std::vec![
                        #(#variant_descriptor_tokens),*
                    ],
                }
            }

            fn encode_payload(&self, __pardosa_buf: &mut ::std::vec::Vec<u8>) -> ::std::result::Result<(), ::pardosa::encoding::EncodeError> {
                match self {
                    #(#encode_match_arms),*
                }
            }

            fn decode_payload(__pardosa_buf: &[u8]) -> ::std::result::Result<Self, ::pardosa::encoding::DecodeError> {
                #decode_disc_extract
                match disc {
                    #(#decode_match_arms,)*
                    other => ::std::result::Result::Err(::pardosa::encoding::DecodeError::UnknownVariantDiscriminant {
                        discriminant: other,
                    }),
                }
            }
        }
        #(#depth_assertions)*
    })
}

fn parse_schema_version(input: &DeriveInput) -> syn::Result<u32> {
    let mut found_version: Option<u32> = None;
    for attr in &input.attrs {
        if attr.path().is_ident("pardosa") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("version") {
                    if found_version.is_some() {
                        return Err(syn::Error::new_spanned(
                            &meta.path,
                            "duplicate `version` attribute on PardosaSchema root",
                        ));
                    }
                    let value = meta.value()?;
                    let lit: syn::LitInt = value.parse()?;
                    let v = lit.base10_parse::<u32>()?;
                    if v == 0 {
                        return Err(syn::Error::new_spanned(
                            &lit,
                            "schema version must be non-zero (version = 0 is reserved per C8.2)",
                        ));
                    }
                    found_version = Some(v);
                    Ok(())
                } else {
                    Ok(())
                }
            })?;
        }
    }
    match found_version {
        Some(v) => Ok(v),
        None => Err(syn::Error::new_spanned(
            input,
            "missing mandatory `#[pardosa(version = N)]` attribute on PardosaSchema root",
        )),
    }
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

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "syn AST expression/type variants"
)]
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

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "syn AST expression/type variants"
)]
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
                if &seg.ident == enum_name || seg.ident == "Self" {
                    return Err(syn::Error::new_spanned(
                        ty,
                        format!(
                            "cycle detected: recursive type `{}` is not permitted in schema descriptor per C6.22 (S4)",
                            seg.ident
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
        let input: DeriveInput = parse_str(
            "#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone, Other = 1 }",
        )
        .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("must have an explicit discriminant"));
    }

    #[test]
    fn test_reject_missing_tombstone() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyEvent { Active = 1, Suspended = 2 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing required event kind: tombstone"));
    }

    #[test]
    fn test_reject_floating_point() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, FloatData(f64) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("floating-point representations are excluded"));
    }

    #[test]
    fn test_reject_unbounded_string() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, StrData(String) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("unsupported type `String`"));
    }

    #[test]
    fn test_reject_unbounded_vec() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, VecData(Vec<u8>) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("unsupported type `Vec`"));
    }

    #[test]
    fn test_detect_cycle_direct() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyNode { #[pardosa(tombstone)] Tombstone = 0, Next(Box<MyNode>) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("cycle detected"));
    }

    #[test]
    fn test_detect_cycle_self() {
        let input: DeriveInput = parse_str("struct Node { children: EventVec<Self, 1> }").unwrap();
        let err = expand_pardosa_type(&input).unwrap_err();
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

    #[test]
    fn test_reject_missing_version_attr() {
        let input: DeriveInput =
            parse_str("enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, Created(u32) = 1 }")
                .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains(
            "missing mandatory `#[pardosa(version = N)]` attribute on PardosaSchema root"
        ));
    }

    #[test]
    fn test_reject_version_zero() {
        let input: DeriveInput = parse_str(
            "#[pardosa(version = 0)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, Created(u32) = 1 }"
        ).unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("schema version must be non-zero"));
    }

    #[test]
    fn test_reject_duplicate_version() {
        let input: DeriveInput = parse_str(
            "#[pardosa(version = 1, version = 2)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, Created(u32) = 1 }"
        ).unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("duplicate `version` attribute"));
    }

    #[test]
    fn test_reject_discriminant_overflow() {
        let input: DeriveInput =
            parse_str("#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, Big = 65536 }").unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("exceeds maximum allowed value 65535"));
    }

    #[test]
    fn test_reject_duplicate_discriminant() {
        let input: DeriveInput = parse_str(
            "#[pardosa(version = 1)] enum MyEvent { #[pardosa(tombstone)] Tombstone = 0, First = 1, Second = 1 }",
        )
        .unwrap();
        let err = expand_pardosa_schema(&input).unwrap_err();
        assert!(err.to_string().contains("duplicate discriminant"));
    }

    #[test]
    fn test_pardosa_type_named_struct_passes() {
        let input: DeriveInput = parse_str("struct Person { id: u32 }").unwrap();
        let res = expand_pardosa_type(&input);
        assert!(res.is_ok());
    }

    #[test]
    fn test_pardosa_type_unit_struct_passes() {
        let input: DeriveInput = parse_str("struct Sentinel;").unwrap();
        let res = expand_pardosa_type(&input);
        assert!(res.is_ok());
    }

    #[test]
    fn test_pardosa_type_scalar_enum_passes() {
        let input: DeriveInput =
            parse_str("enum Status { Inactive = 0, Active = 1, Suspended = 2 }").unwrap();
        let res = expand_pardosa_type(&input);
        assert!(res.is_ok());
    }

    #[test]
    fn test_pardosa_type_composite_enum_passes() {
        let input: DeriveInput =
            parse_str("enum Message { Empty = 0, Data(u64) = 1, Detail { code: u16 } = 2 }")
                .unwrap();
        let res = expand_pardosa_type(&input);
        assert!(res.is_ok());
    }

    #[test]
    fn test_pardosa_type_reject_union() {
        let input: DeriveInput = parse_str("union Data { x: u32 }").unwrap();
        let err = expand_pardosa_type(&input).unwrap_err();
        assert!(err.to_string().contains("union is not supported"));
    }

    #[test]
    fn test_pardosa_type_reject_missing_discriminant() {
        let input: DeriveInput = parse_str("enum Status { First, Second = 1 }").unwrap();
        let err = expand_pardosa_type(&input).unwrap_err();
        assert!(err
            .to_string()
            .contains("must have an explicit discriminant"));
    }

    #[test]
    fn test_pardosa_type_reject_unbounded_type() {
        let input: DeriveInput = parse_str("struct Bad { text: String }").unwrap();
        let err = expand_pardosa_type(&input).unwrap_err();
        assert!(err.to_string().contains("unsupported type `String`"));
    }
}
