enum Regex {
    Literal(String),
}

enum ParseError {
    InvalidRegex(String),
}

fn parse_regex(regex: &str) -> Result<Regex, ParseError> {
    Ok(Regex::Literal(regex.to_string()))
}

fn match_regex(regex: Regex, input: &str) -> Result<bool, ParseError> {
    match regex {
        Regex::Literal(literal) => Ok(input.contains(&literal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blessed::JsonSchema;

    #[derive(Serialize)]
    struct SerializableAST(Regex);

    #[derive(JsonSchema)]
    struct Case {
        regex: String,
        inputs: Vec<String>,
    }

    #[derive(Serialize)]
    struct Output {
        parse_error: Option<String>,
        ast: SerializableAST,
        matches: Map<String, bool>,
    }

    #[blessed::harness]
    fn parse_compile_match(case: Case) -> Output {
        let parsed = parse_regex(&case.regex);
        match parsed {
            Ok(ast) => Output {
                ast: SerializableAST(ast),
                parse_error: None,
                matches: case
                    .inputs
                    .iter()
                    .map(|input| (input.clone(), match_regex(ast, input)))
                    .collect(),
            },
            Err(e) => Output {
                ast: SerializableAST(Literal("".to_string())),
                parse_error: Some(e.to_string()),
                matches: vec![],
            },
        }
    }

    blessed::tests!("tests/", "tests/blessed/");
}
