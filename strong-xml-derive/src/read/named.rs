use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

use crate::types::{Field, Type};

pub fn read(tag: &LitStr, ele_name: TokenStream, fields: &Vec<Field>) -> TokenStream {
    let init_fields = fields.iter().map(|field| match field {
        Field::Attribute { name, ty, .. }
        | Field::Child { name, ty, .. }
        | Field::FlattenText { name, ty, .. } => init_value(name, ty),
        Field::Text { name, .. } => quote! { let #name; },
    });

    let return_fields = fields.iter().map(|field| match field {
        Field::Attribute {
            name, ty, default, ..
        }
        | Field::Child {
            name, ty, default, ..
        }
        | Field::FlattenText {
            name, ty, default, ..
        } => return_value(name, ty, *default, &ele_name),
        Field::Text { name, ty } => return_value(name, ty, false, &ele_name),
    });

    let read_attr_fields = fields.iter().filter_map(|field| match field {
        Field::Attribute { name, ty, tag, .. } => Some(read_attrs(&tag, &name, &ty, &ele_name)),
        _ => None,
    });

    let read_child_fields = fields.iter().filter_map(|field| match field {
        Field::Child { name, ty, tags, .. } => Some(read_children(tags, name, ty, &ele_name)),
        _ => None,
    });

    let read_flatten_text_fields = fields.iter().filter_map(|field| match field {
        Field::FlattenText { name, ty, tag, .. } => {
            Some(read_flatten_text(tag, name, ty, &ele_name))
        }
        _ => None,
    });

    let read_text_fields = fields.iter().filter_map(|field| match field {
        Field::Text { name, ty } => Some(read_text(&tag, name, ty, &ele_name)),
        _ => None,
    });

    let is_text_element = fields
        .iter()
        .any(|field| matches!(field, Field::Text { .. }));

    let return_fields = quote! {
        let __res = #ele_name {
            #( #return_fields, )*
        };

        strong_xml::log_finish_reading!(#ele_name);

        return Ok(__res);
    };

    let read_content = if is_text_element {
        quote! {
            #( #read_text_fields )*
            #return_fields
        }
    } else {
        quote! {
            if let Token::ElementEnd { end: ElementEnd::Empty, .. } = reader.next().unwrap()? {
                #return_fields
            }

            while let Some(__tag) = reader.find_element_start(Some(#tag))? {
                match __tag {
                    #( #read_child_fields, )*
                    #( #read_flatten_text_fields, )*
                    tag => {
                        strong_xml::log_skip_element!(#ele_name, tag);
                        // skip the start tag
                        reader.next();
                        reader.read_to_end(tag)?;
                    },
                }
            }

            #return_fields
        }
    };

    quote! {
        strong_xml::log_start_reading!(#ele_name);

        #( #init_fields )*

        reader.read_till_element_start(#tag)?;

        while let Some((__key, __value)) = reader.find_attribute()? {
            match __key {
                #( #read_attr_fields, )*
                key => {
                    strong_xml::log_skip_attribute!(#ele_name, key);
                },
            }
        }

        #read_content
    }
}

fn init_value(name: &Ident, ty: &Type) -> TokenStream {
    if ty.is_vec() {
        quote! { let mut #name = Vec::new(); }
    } else {
        quote! { let mut #name = None; }
    }
}

fn return_value(name: &Ident, ty: &Type, default: bool, ele_name: &TokenStream) -> TokenStream {
    if ty.is_vec() || ty.is_option() {
        quote! { #name }
    } else if default {
        quote! { #name: #name.unwrap_or_default() }
    } else {
        quote! {
            #name: #name.ok_or(XmlError::MissingField {
                name: stringify!(#ele_name).to_owned(),
                field: stringify!(#name).to_owned(),
            })?
        }
    }
}

fn read_attrs(tag: &LitStr, name: &Ident, ty: &Type, ele_name: &TokenStream) -> TokenStream {
    let from_str = from_str(ty);

    if ty.is_vec() {
        panic!("`attr` attribute doesn't support Vec.");
    } else {
        quote! {
            #tag => {
                strong_xml::log_start_reading_field!(#ele_name, #name);

                #name = Some(#from_str);

                strong_xml::log_finish_reading_field!(#ele_name, #name);
            }
        }
    }
}

fn read_text(tag: &LitStr, name: &Ident, ty: &Type, ele_name: &TokenStream) -> TokenStream {
    let from_str = from_str(ty);

    if ty.is_vec() {
        panic!("`text` attribute doesn't support Vec.");
    } else {
        quote! {
            strong_xml::log_start_reading_field!(#ele_name, #name);

            let __value = reader.read_text(#tag)?;
            #name = Some(#from_str);

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn read_children(tags: &[LitStr], name: &Ident, ty: &Type, ele_name: &TokenStream) -> TokenStream {
    let from_reader = match &ty {
        Type::VecT(ty) => quote! {
            #name.push(<#ty as strong_xml::XmlRead>::from_reader(reader)?);
        },
        Type::OptionT(ty) | Type::T(ty) => quote! {
            #name = Some(<#ty as strong_xml::XmlRead>::from_reader(reader)?);
        },
        _ => panic!("`child` attribute only supports Vec<T>, Option<T> and T."),
    };

    quote! {
        #( #tags )|* => {
            strong_xml::log_start_reading_field!(#ele_name, #name);

            #from_reader

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn read_flatten_text(tag: &LitStr, name: &Ident, ty: &Type, ele_name: &TokenStream) -> TokenStream {
    let from_str = from_str(ty);

    let read_text = if ty.is_vec() {
        quote! {
            let __value = reader.read_text(#tag)?;
            #name.push(#from_str);
        }
    } else {
        quote! {
            let __value = reader.read_text(#tag)?;
            #name = Some(#from_str);
        }
    };

    quote! {
        #tag => {
            // skip element start
            reader.next();

            strong_xml::log_start_reading_field!(#ele_name, #name);

            #read_text

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn from_str(ty: &Type) -> TokenStream {
    match &ty {
        Type::CowStr | Type::OptionCowStr | Type::VecCowStr => quote! { __value },
        Type::Bool | Type::OptionBool | Type::VecBool => quote! {
            match &*__value {
                "t" | "true" | "y" | "yes" | "on" | "1" => true,
                "f" | "false" | "n" | "no" | "off" | "0" => false,
                _ => <bool as std::str::FromStr>::from_str(&__value).map_err(|e| XmlError::FromStr(e.into()))?
            }
        },
        Type::T(ty) | Type::OptionT(ty) | Type::VecT(ty) => quote! {
            <#ty as std::str::FromStr>::from_str(&__value).map_err(|e| XmlError::FromStr(e.into()))?
        },
    }
}
