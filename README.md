# Blessed - Gold Testing for Humans

`blessed` is a tool for writing, running, and maintaining gold tests.

All you have to do to write a gold test is add a `.blessed.json` file to `src/tests/`, write a simple test harness that accepts a JSON-serializable input and returns a JSON-serializable output, and add two lines to `build.rs` script. Blessed will generate a regular rust unit test for every case in `src/tests/*.blessed.json`. The test runs the case using the harness, writes the results to `blessed/{test_name}.json`, and then fails if this json file doesn't match its git staged version.

## `src/lib.rs`

```rust
fn system_under_tests(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    use blessed::{HarnessFn, Inventory, Serialize};
    use schemars::JsonSchema;

    #[derive(JsonSchema, Deserialize)]
    struct Case {
        a: i32,
        b: i32,
    }

    #[derive(Serialize)]
    struct Output {
        result: i32,
    }

    #[blessed::harness]
    fn my_harness(case: Case) -> Output {
        let result = system_under_tests(case.a, case.b);
        Output { result }
    }
}
```

## `build.rs`

Create a `build.rs` file in your project root (if it doesn't exist) and add these lines:

```rust
fn main() {
    // ...
    println!("cargo:rerun-if-changed=src/tests/"); // rebuild if a test changes
    println!("cargo:rerun-if-changed=build.rs"); // rebuild if build.rs changes
}
```

Make sure to add `blessed` to your `[dev-dependencies]` and `[build-dependencies]` in `Cargo.toml`:

```bash
cargo add --dev blessed
cargo add --build blessed
```

## `src/tests/tests.blessed.json`

```json
{
    "happy": {
        "harness": "my_harness",
        "params": {
            "a": 1,
            "b": 2
        }
    },
    "large_numbers": {
        "harness": "my_harness",
        "params": {
            "a": 1000000,
            "b": 2000000
        }
    },
}
```

## Running the tests

```bash
cargo test
```
