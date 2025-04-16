use glob;
use proc_macro::TokenStream;
use quote::quote;
use serde::Deserialize;
use serde_json::{self, Value as JsonValue};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use syn::{parse_macro_input, punctuated::Punctuated, Ident, ItemFn, LitStr, PatType, Token};

#[derive(Deserialize, Debug)]
struct BlessedDefinition {
    harness: String,
    params: JsonValue,
}

// Intermediate struct to hold processed test information
#[derive(Debug)]
struct PreparedTest {
    test_fn_name: Ident,
    test_name: String,
    harness_name: String,
    params: JsonValue,
    output_file_path_rel_str: String,
}

// Struct to hold common paths
struct ProjectPaths {
    git_root: PathBuf,
    git_root_str: String,
    output_dir_abs: PathBuf,
    glob_pattern_str: String,
}

#[proc_macro_attribute]
pub fn harness(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: Write test case for every panic here

    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();

    // Extract input argument type
    let input_arg = func
        .sig
        .inputs
        .first()
        .expect("Harness function must have exactly one argument");
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
    let wrapper_func_name = Ident::new(
        &format!("__blessed_harness_{}", func_name),
        func_name.span(),
    );

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

// Helper function to find git root and related paths
fn find_project_paths() -> Result<ProjectPaths, syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map_err(|_| {
            syn::Error::new(proc_macro2::Span::call_site(), "CARGO_MANIFEST_DIR not set")
        })?;

    let git_root_output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&manifest_dir)
        .output()
        .map_err(|e| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "Failed to execute git command: {}. Is git installed and in PATH?",
                    e
                ),
            )
        })?;

    if !git_root_output.status.success() {
        let stderr = String::from_utf8_lossy(&git_root_output.stderr);
        let msg = format!(
            "`git rev-parse --show-toplevel` failed (exit code: {}): {}",
            git_root_output.status, stderr
        );
        return Err(syn::Error::new(proc_macro2::Span::call_site(), msg));
    }

    let git_root_str = String::from_utf8_lossy(&git_root_output.stdout)
        .trim()
        .to_string();
    let git_root = PathBuf::from(&git_root_str);

    if git_root_str.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "Failed to determine git root directory",
        ));
    }
    if !git_root.is_absolute() {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), format!("Determined git root path is not absolute: {:?}. Blessed requires an absolute path.", git_root)));
    }
    let git_root_str_final = git_root
        .to_str()
        .ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Git root path is not valid UTF-8: {:?}", git_root),
            )
        })?
        .to_string();

    let output_dir_abs = manifest_dir.join("blessed/");
    let absolute_glob_pattern = manifest_dir.join("src/**/*.blessed.json");
    let glob_pattern_str = absolute_glob_pattern
        .to_str()
        .ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "Glob pattern path is not valid UTF-8: {:?}",
                    absolute_glob_pattern
                ),
            )
        })?
        .to_string();

    Ok(ProjectPaths {
        git_root,
        git_root_str: git_root_str_final,
        output_dir_abs,
        glob_pattern_str,
    })
}

// Helper function to collect test definitions from files
fn collect_test_definitions(paths: &ProjectPaths) -> Result<(Vec<PreparedTest>, bool), syn::Error> {
    let mut prepared_tests = Vec::new();
    let mut found_files = false;

    eprintln!(
        "Searching for blessed files using glob: {}",
        paths.glob_pattern_str
    );

    match glob::glob(&paths.glob_pattern_str) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(input_json_path) => {
                        if !input_json_path.is_file() {
                            continue;
                        }
                        found_files = true;
                        eprintln!("Processing blessed definition file: {:?}", input_json_path);

                        let file_stem = input_json_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|stem| stem.replace(|c: char| !c.is_alphanumeric(), "_"))
                            .ok_or_else(|| {
                                syn::Error::new(
                                    proc_macro2::Span::call_site(),
                                    format!(
                                        "Could not get file stem from path: {:?}",
                                        input_json_path
                                    ),
                                )
                            })?;

                        let file_content = fs::read_to_string(&input_json_path).map_err(|e| {
                            syn::Error::new(
                                proc_macro2::Span::call_site(),
                                format!("Failed to read blessed file {:?}: {}", input_json_path, e),
                            )
                        })?;

                        // TODO: Implement advanced test authoring features here by processing the raw cases
                        let test_cases: HashMap<String, BlessedDefinition> =
                            serde_json::from_str(&file_content).map_err(|e| {
                                syn::Error::new(
                                    proc_macro2::Span::call_site(),
                                    format!(
                                        "Failed to parse blessed file {:?}: {}",
                                        input_json_path, e
                                    ),
                                )
                            })?;

                        for (test_name, definition) in test_cases {
                            let test_fn_name = Ident::new(
                                &format!("blessed_test_{}_{}", file_stem, test_name),
                                proc_macro2::Span::call_site(),
                            );
                            let output_file_name = format!("{}.json", test_name);
                            let output_file_path_abs = paths.output_dir_abs.join(&output_file_name);

                            let output_file_path_rel = output_file_path_abs
                                .strip_prefix(&paths.git_root)
                                .map_err(|_| {
                                    syn::Error::new(
                                        test_fn_name.span(),
                                        format!(
                                            "Output file path {:?} is not inside git root {:?}",
                                            output_file_path_abs, paths.git_root
                                        ),
                                    )
                                })?
                                .to_path_buf();

                            let output_file_path_rel_str = output_file_path_rel
                                .to_str()
                                .ok_or_else(|| {
                                    syn::Error::new(
                                        test_fn_name.span(),
                                        format!(
                                            "Relative output path is not valid UTF-8: {:?}",
                                            output_file_path_rel
                                        ),
                                    )
                                })?
                                .to_string();

                            prepared_tests.push(PreparedTest {
                                test_fn_name,
                                test_name: test_name.clone(),
                                harness_name: definition.harness,
                                params: definition.params,
                                output_file_path_rel_str,
                            });
                        }
                    }
                    Err(e) => {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!("Error processing glob entry: {}", e),
                        ));
                    }
                }
            }
        }
        Err(e) => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "Failed to read glob pattern '{}': {}",
                    paths.glob_pattern_str, e
                ),
            ));
        }
    }

    Ok((prepared_tests, found_files))
}

// Helper function to generate code for a single test function
fn generate_test_function_code(
    prep: PreparedTest,
    git_root_path_str: &str,
    output_dir_abs_str: &str,
) -> proc_macro2::TokenStream {
    let test_fn_name = prep.test_fn_name;
    let test_name_str = prep.test_name;
    let harness_name = prep.harness_name;
    let params_value = prep.params;
    let output_file_path_rel_str = prep.output_file_path_rel_str;

    let params_json_str_lit = params_value.to_string();
    let output_file_name = format!("{}.json", test_name_str);

    // Pass owned Strings to quote! macro to avoid lifetime issues if needed
    let git_root_path_str = git_root_path_str.to_string();
    let output_dir_abs_str = output_dir_abs_str.to_string();

    quote! {
        #[test]
        fn #test_fn_name() {
            let harness_name = #harness_name;
            let params_json_str = #params_json_str_lit;
            let params: ::serde_json::Value = ::serde_json::from_str(params_json_str)
                 .expect("Internal error: Failed to re-parse params JSON string");

            let output_file_name = #output_file_name;
            let output_dir_abs_str = #output_dir_abs_str;
            let output_file_path_rel_str = #output_file_path_rel_str;
            let git_root_path_str = #git_root_path_str;

            let output_path_abs = ::std::path::Path::new(output_dir_abs_str).join(output_file_name);

            let harness = match ::inventory::iter::<::blessed::HarnessFn>
                .into_iter()
                .find(|h| h.name == harness_name)
            {
                Some(h) => h,
                None => panic!("Blessed harness function '{}' not found. Available: {:?}",
                                 harness_name,
                                 ::inventory::iter::<::blessed::HarnessFn>.into_iter().map(|h| h.name).collect::<Vec<_>>())
            };

            let result = (harness.func)(params);
            let output_json = match result {
                Ok(value) => ::serde_json::to_string_pretty(&value).expect("Failed to serialize result to JSON"),
                Err(e) => {
                    let error_output = ::serde_json::json!({ "blessed_error": e });
                    ::serde_json::to_string_pretty(&error_output).expect("Failed to serialize error to JSON")
                }
            };

            // Write Output File
            if let Some(parent) = output_path_abs.parent() {
                ::std::fs::create_dir_all(parent).unwrap_or_else(|e|
                    panic!("Failed to create output directory '{:?}': {}", parent, e)
                );
            }
            ::std::fs::write(&output_path_abs, &output_json).unwrap_or_else(|e|
                panic!("Failed to write blessed output file '{:?}': {}", output_path_abs, e)
            );

            // Check Git Status
            match run_git_status(git_root_path_str, output_file_path_rel_str) {
                Ok(status_output) => {
                    let status_trimmed = status_output.trim_start();

                    if status_trimmed.starts_with("??") {
                        panic!("Blessed test '{}': Untracked file '{}'. Please review and `git add` the file.",
                                 #test_name_str, output_file_path_rel_str);
                    } else if status_trimmed.starts_with("M") || status_trimmed.starts_with("AM") {
                        panic!("Blessed test '{}': File '{}' is modified and differs from the git index. Please review changes and `git add` or revert.",
                                 #test_name_str, output_file_path_rel_str);
                    } else if status_trimmed.starts_with("A") || status_output.trim().is_empty() {
                        // Test passes.
                    } else if !status_output.trim().is_empty() {
                        panic!("Blessed test '{}': Unexpected git status for '{}': {:?}. Please check repository state.",
                                 #test_name_str, output_file_path_rel_str, status_output);
                    }
                }
                Err(e) => {
                    panic!("Blessed test '{}': Failed to get git status for '{}': {}",
                             #test_name_str, output_file_path_rel_str, e);
                }
            }
        }
    }
}

#[proc_macro]
pub fn tests(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input with Punctuated::<LitStr, Token![,]>::parse_terminated);
    if !args.is_empty() {
        return syn::Error::new_spanned(args, "No arguments expected")
            .to_compile_error()
            .into();
    }

    let paths = match find_project_paths() {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    let (prepared_tests, found_files) = match collect_test_definitions(&paths) {
        Ok(result) => result,
        Err(e) => return e.to_compile_error().into(),
    };

    let final_code = if !found_files {
        // Generate a single failing test if no files were found
        let error_message = format!(
            "Blessed error: No test definition files found matching glob pattern '{}'",
            paths.glob_pattern_str
        );
        quote! {
            #[test]
            fn blessed_no_files_found() {
                panic!(#error_message);
            }
        }
    } else {
        // Proceed with generating tests if files were found
        let num_tests = prepared_tests.len();
        let output_dir_abs_str = paths
            .output_dir_abs
            .to_str()
            .expect("Output dir path not valid UTF-8")
            .to_string();

        // Define the helper function once
        let run_git_status_fn = quote! {
                #[doc(hidden)]
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
        };

        let generated_tests = prepared_tests.into_iter().map(|prep| {
            generate_test_function_code(prep, &paths.git_root_str, &output_dir_abs_str)
        });

        eprintln!("Generated {} blessed tests.", num_tests);

        quote! {
            #run_git_status_fn // Include the helper function definition
            #(#generated_tests)*
        }
    };

    TokenStream::from(final_code)
}
