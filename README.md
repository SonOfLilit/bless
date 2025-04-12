# Blessed - Gold Testing for Humans

`blessed` is a tool for writing, running, and maintaining gold tests.

All you have to do to write a gold test is add a `.blessed.json` file to your test directory and write a simple test harness that accepts a JSON-serializable input, does something, and returns a JSON-serializable output.

## `src/lib.rs`

```rust
fn system_under_tests(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    use blessed::JsonSchema;

    #[derive(JsonSchema)]
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

    blessed::tests!("tests/", "tests/blessed/");
}
```

## `src/tests/tests.blessed.json`

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

Blessed will generate a regular rust unit test for every case in the `tests.blessed.json` file. The test runs the case using the harness, writes the results to `tests/blessed/{test_name}.json`, and then fails if this json file doesn't match its git staged version.

## Running the tests

```bash
cargo test
```
