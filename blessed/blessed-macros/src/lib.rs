use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, Ident, Type, PatType, LitStr, Token, punctuated::Punctuated};
use std::fs;
use std::path::Path;
use serde::Deserialize;
use serde_json;
use std::collections::HashMap;
use glob;

#[derive(Deserialize, Debug)]
struct BlessedDefinition {
    harness: String,
    params: serde_json::Value,
}

#[proc_macro_attribute]
pub fn harness(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: Write test case for every panic here

    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();

    // Extract input argument type
    let input_arg = func.sig.inputs.first().expect("Harness function must have exactly one argument");
    let input_type = match input_arg {
        syn::FnArg::Typed(PatType { ty, .. }) => ty,
        _ => panic!("Harness function argument must be typed"),
    };

    // Extract return type
    let output_type = match &func.sig.output {
        syn::ReturnType::Type(_, ty) => ty,
        _ => panic!("Harness function must have a return type"),
    };

    // Generate the wrapper function name
    let wrapper_func_name = Ident::new(&format!("__blessed_harness_{}", func_name), func_name.span());

    let generated_code = quote! {
        #func // Keep the original function definition

        #[doc(hidden)]
        fn #wrapper_func_name(input_json: ::serde_json::Value) -> Result<::serde_json::Value, String> {
            let input: #input_type = ::serde_json::from_value(input_json)
                .map_err(|e| format!("Failed to deserialize input: {}", e))?;

            let output: #output_type = #func_name(input);

            ::serde_json::to_value(output)
                .map_err(|e| format!("Failed to serialize output: {}", e))
        }

        ::inventory::submit! {
            ::blessed::HarnessFn {
                name: #func_name_str,
                func: #wrapper_func_name,
            }
        }
    };

    TokenStream::from(generated_code)
}

#[proc_macro]
pub fn tests(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input with Punctuated::<LitStr, Token![,]>::parse_terminated);
    if args.len() != 2 {
        return syn::Error::new_spanned(args, "Expected exactly two string literal arguments: input JSON glob pattern and output directory")
            .to_compile_error()
            .into();
    }
    let input_glob_pattern_rel = args[0].value();
    let output_dir_rel = args[1].value();

    let mut generated_tests = Vec::new();

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(_) => return syn::Error::new(proc_macro2::Span::call_site(), "CARGO_MANIFEST_DIR not set").to_compile_error().into(),
    };
    let absolute_glob_pattern = Path::new(&manifest_dir).join(&input_glob_pattern_rel);
    let output_dir_abs = Path::new(&manifest_dir).join(&output_dir_rel);

    eprintln!("Searching for blessed files with pattern: {:?}", absolute_glob_pattern);

    let glob_pattern_str = match absolute_glob_pattern.to_str() {
        Some(s) => s,
        None => {
            let msg = format!("Invalid glob pattern path: {:?}", absolute_glob_pattern);
            return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
        }
    };

    let mut found_files = false;
    match glob::glob(glob_pattern_str) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(input_json_path) => {
                        if input_json_path.is_file() {
                            found_files = true;
                            eprintln!("Processing blessed definition file: {:?}", input_json_path);

                            let file_stem = match input_json_path.file_stem().and_then(|s| s.to_str()) {
                                Some(stem) => stem.replace(|c: char| !c.is_alphanumeric(), "_"), // Sanitize stem for ident
                                None => {
                                    let msg = format!("Could not get file stem from path: {:?}", input_json_path);
                                    return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                }
                            };

                            let file_content = match fs::read_to_string(&input_json_path) {
                                Ok(content) => content,
                                Err(e) => {
                                    let msg = format!("Failed to read blessed file {:?}: {}", input_json_path, e);
                                    return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                }
                            };

                            let test_cases: Result<HashMap<String, BlessedDefinition>, _> = serde_json::from_str(&file_content);

                            match test_cases {
                                Ok(cases) => {
                                    for (test_name, definition) in cases {
                                        // Include file stem in test function name
                                        let test_fn_name = Ident::new(&format!("blessed_test_{}_{}", file_stem, test_name), proc_macro2::Span::call_site());
                                        let harness_name = definition.harness;
                                        let params_json_str = definition.params.to_string();
                                        // Use absolute output dir path
                                        let output_file_path = output_dir_abs.join(format!("{}.json", test_name));
                                        let output_file_path_str = match output_file_path.to_str() {
                                            Some(s) => s.to_string(),
                                            None => {
                                               let msg = format!("Invalid output path generated: {:?}", output_file_path);
                                               return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                            }
                                        };

                                        generated_tests.push(quote! {
                                            #[test]
                                            fn #test_fn_name() {
                                                let harness_name = #harness_name;
                                                let params_json_str = #params_json_str;
                                                let output_file_path = #output_file_path_str;

                                                let output_path = ::std::path::Path::new(output_file_path);
                                                let existing_golden = ::std::fs::read_to_string(output_path).ok();

                                                let harness = match ::inventory::iter::<::blessed::HarnessFn>
                                                    .into_iter()
                                                    .find(|h| h.name == harness_name)
                                                {
                                                    Some(h) => h,
                                                    None => panic!("Blessed harness function '{}' not found. Available: {:?}",
                                                                   harness_name,
                                                                   ::inventory::iter::<::blessed::HarnessFn>.into_iter().map(|h| h.name).collect::<Vec<_>>())
                                                };

                                                let params: ::serde_json::Value = ::serde_json::from_str(params_json_str)
                                                    .expect("Failed to parse params JSON string (should not happen)");

                                                let result = (harness.func)(params);

                                                let output_json = match result {
                                                    Ok(value) => ::serde_json::to_string_pretty(&value).expect("Failed to serialize result to JSON"),
                                                    Err(e) => {
                                                        let error_output = ::serde_json::json!({ "blessed_error": e });
                                                        ::serde_json::to_string_pretty(&error_output).expect("Failed to serialize error to JSON")
                                                    }
                                                };

                                                if let Some(parent) = output_path.parent() {
                                                    if let Err(e) = ::std::fs::create_dir_all(parent) {
                                                        panic!("Failed to create output directory '{:?}': {}", parent, e);
                                                    }
                                                }

                                                if let Err(e) = ::std::fs::write(output_path, &output_json) {
                                                    panic!("Failed to write blessed output file '{}': {}", output_file_path, e);
                                                }

                                                match existing_golden {
                                                    Some(golden_content) => {
                                                        if output_json != golden_content {
                                                            panic!("Blessed test '{}' failed:\n--- Golden File: {}\n+++ Output:\n{}", #test_name, output_file_path, output_json);
                                                        }
                                                    }
                                                    None => {
                                                        eprintln!("Blessed test '{}': Golden file created at {}. Please commit it.", #test_name, output_file_path);
                                                        panic!("Blessed test '{}': Golden file created at {}. Please commit it.", #test_name, output_file_path);
                                                    }
                                                }
                                            }
                                        });
                                    }
                                },
                                Err(e) => {
                                    let msg = format!("Failed to parse blessed file {:?}: {}", input_json_path, e);
                                    return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                }
                            }
                        }
                    },
                    Err(e) => {
                        // Handle glob error entry (e.g., permission denied)
                        let msg = format!("Error processing glob entry: {}", e);
                        eprintln!("{}", msg);
                    }
                }
            }
        },
        Err(e) => {
            // Handle error during glob pattern processing itself
            let msg = format!("Failed to read glob pattern '{}': {}", glob_pattern_str, e);
            return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
        }
    }

    // TODO: Generate one failing test so this behaves like a test failure
    if !found_files {
         eprintln!("Warning: No blessed files found matching pattern: {:?}", absolute_glob_pattern);
    }

    let final_code = quote! {
        #(#generated_tests)*
    };

    eprintln!("Generated {} blessed tests from pattern '{}'.", generated_tests.len(), input_glob_pattern_rel);

    TokenStream::from(final_code)
} 