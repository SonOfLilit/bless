use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, Ident, PatType, LitStr, Token, punctuated::Punctuated};
use std::fs;
use std::path::PathBuf;
use serde::Deserialize;
use serde_json;
use std::collections::HashMap;
use glob;
use std::process::Command;

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
    if args.len() != 0 {
        return syn::Error::new_spanned(args, "No arguments expected")
            .to_compile_error()
            .into();
    }

    let mut generated_tests = Vec::new();

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return syn::Error::new(proc_macro2::Span::call_site(), "CARGO_MANIFEST_DIR not set").to_compile_error().into(),
    };
    let absolute_glob_pattern = manifest_dir.join("src/**/*.blessed.json");
    let output_dir_abs = manifest_dir.join("blessed/");

    // --- Find Git Root --- 
    let git_root = match Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&manifest_dir)
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            PathBuf::from(stdout)
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = format!("`git rev-parse --show-toplevel` failed (exit code: {}): {}", output.status, stderr);
            return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
        }
        Err(e) => {
            let msg = format!("Failed to execute git command: {}. Is git installed and in PATH?", e);
            return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
        }
    };
    let git_root_str = match git_root.to_str() {
         Some(s) => s.to_string(),
         None => {
             let msg = format!("Git root path is not valid UTF-8: {:?}", git_root);
             return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
         }
    };
    // --- End Find Git Root ---

    eprintln!("Searching for blessed files");

    let glob_pattern_str = absolute_glob_pattern.to_str().unwrap();

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
                                        let output_file_path_abs = output_dir_abs.join(format!("{}.json", test_name));

                                        // --- Calculate Relative Path ---
                                        let output_file_path_rel = match output_file_path_abs.strip_prefix(&git_root) {
                                            Ok(p) => p.to_path_buf(),
                                            Err(_) => {
                                                let msg = format!("Output file path {:?} is not inside git root {:?}", output_file_path_abs, git_root);
                                                return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                            }
                                        };
                                        let output_file_path_rel_str = match output_file_path_rel.to_str() {
                                            Some(s) => s.to_string(),
                                            None => {
                                                let msg = format!("Relative output path is not valid UTF-8: {:?}", output_file_path_rel);
                                                return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                            }
                                        };
                                        // --- End Calculate Relative Path ---

                                        let output_file_path_abs_str = match output_file_path_abs.to_str() {
                                            Some(s) => s.to_string(),
                                            None => {
                                               let msg = format!("Absolute output path is not valid UTF-8: {:?}", output_file_path_abs);
                                               return syn::Error::new(proc_macro2::Span::call_site(), msg).to_compile_error().into();
                                            }
                                        };

                                        // Clone git_root_str for use inside quote!
                                        let git_root_str_clone = git_root_str.clone();

                                        generated_tests.push(quote! {
                                            #[test]
                                            fn #test_fn_name() {
                                                let harness_name = #harness_name;
                                                let params_json_str = #params_json_str;
                                                let output_file_path_abs_str = #output_file_path_abs_str;
                                                let output_file_path_rel_str = #output_file_path_rel_str;
                                                let git_root_path_str = #git_root_str_clone; // Use cloned git root

                                                // --- Helper Fn: Run Git Status ---
                                                fn run_git_status(git_root: &str, relative_path: &str) -> Result<String, String> {
                                                    let output = ::std::process::Command::new("git")
                                                        .args(["status", "--porcelain", "--", relative_path])
                                                        .current_dir(git_root)
                                                        .output()
                                                        .map_err(|e| format!("Failed to execute git status: {}", e))?;

                                                    if !output.status.success() {
                                                        let stderr = String::from_utf8_lossy(&output.stderr);
                                                        return Err(format!("`git status` failed (exit code: {}): {}", output.status, stderr));
                                                    }
                                                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                                                }
                                                // --- End Helper Fn ---

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

                                                // Run Harness
                                                let result = (harness.func)(params);
                                                let output_json = match result {
                                                    Ok(value) => ::serde_json::to_string_pretty(&value).expect("Failed to serialize result to JSON"),
                                                    Err(e) => {
                                                        let error_output = ::serde_json::json!({ "blessed_error": e });
                                                        ::serde_json::to_string_pretty(&error_output).expect("Failed to serialize error to JSON")
                                                    }
                                                };

                                                // Write Output File
                                                let output_path_abs = ::std::path::Path::new(output_file_path_abs_str);
                                                if let Some(parent) = output_path_abs.parent() {
                                                    if let Err(e) = ::std::fs::create_dir_all(parent) {
                                                        panic!("Failed to create output directory '{:?}': {}", parent, e);
                                                    }
                                                }
                                                if let Err(e) = ::std::fs::write(output_path_abs, &output_json) {
                                                    panic!("Failed to write blessed output file '{}': {}", output_file_path_abs_str, e);
                                                }

                                                // Check Git Status
                                                match run_git_status(git_root_path_str, output_file_path_rel_str) {
                                                    Ok(status_output) => {
                                                        eprintln!("Raw Git status for '{}': {:?}", output_file_path_rel_str, status_output);

                                                        if status_output.starts_with("?? ") { // Check prefix including space
                                                            panic!("Blessed test '{}': Untracked file '{}'. Please review and `git add` the file.",
                                                                   #test_name, output_file_path_rel_str);
                                                        } else if status_output.starts_with(" M ") || status_output.starts_with("AM ") { // Check prefix including space
                                                            panic!("Blessed test '{}': File '{}' is modified and differs from the git index. Please review changes and `git add` or revert.",
                                                                   #test_name, output_file_path_rel_str);
                                                        } else if status_output.starts_with("A ") || status_output.is_empty() { // Check prefix including space or empty
                                                            // File is unmodified (empty output) or staged and unmodified (`A `)
                                                            // Test passes.
                                                        } else {
                                                            // Capture unexpected non-empty output
                                                            panic!("Blessed test '{}': Unexpected git status for '{}': {:?}. Please check repository state.",
                                                                   #test_name, output_file_path_rel_str, status_output);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        panic!("Blessed test '{}': Failed to get git status for '{}': {}",
                                                               #test_name, output_file_path_rel_str, e);
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

    // TODO: Generate one always-failing test so this presents as a test failure
    if !found_files {
         eprintln!("Warning: No blessed files (src/**/*.blessed.json) found");
    }

    let final_code = quote! {
        #(#generated_tests)*
    };

    eprintln!("Generated {} blessed tests.", generated_tests.len());

    TokenStream::from(final_code)
} 