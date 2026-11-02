/*
 * preproc/parser_functions/expression.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use super::{WikidotParserFunctionOptions, WikidotZeroOperatorPolicy};

const MAX_EXPRESSION_BYTES: usize = 16 * 1024;
const MAX_OPERATIONS: usize = 512;
const MAX_PARENTHESES: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExpressionError {
    Invalid,
    UndefinedConstant(String),
    UndefinedFunction(String),
    TooFewParameters(&'static str),
    TooFewArguments(String, usize, usize),
    TooManyArguments(String, usize, usize),
    MisplacedComma,
    TooManyValues,
    SyntaxNear(String),
    DivisionByZero,
    RemainderByZero,
}

impl ExpressionError {
    pub(super) fn runtime_message(&self) -> Option<String> {
        match self {
            Self::Invalid => None,
            Self::UndefinedConstant(name) => {
                Some(format!(r#"run-time error: undefined constant "{name}""#))
            }
            Self::UndefinedFunction(name) => {
                Some(format!(r#"run-time error: undefined function "{name}""#))
            }
            Self::TooFewParameters(operator) => Some(format!(
                r#"run-time error: too few parameters for operator "{operator}" (2 -> 1)"#,
            )),
            Self::TooFewArguments(function, expected, actual) => Some(format!(
                r#"run-time error: too few arguments for function "{function}"({expected} -> {actual})"#,
            )),
            Self::TooManyArguments(function, expected, actual) => Some(format!(
                r#"run-time error: too many arguments for function "{function}"({expected} -> {actual})"#,
            )),
            Self::MisplacedComma => {
                Some("parser error: missing token `(` or misplaced token `,`".to_owned())
            }
            Self::TooManyValues => {
                Some("run-time error: too many values in the stack".to_owned())
            }
            Self::SyntaxNear(tail) => Some(format!("syntax error near `{tail}`")),
            Self::DivisionByZero => Some("run-time error: division by zero".to_owned()),
            Self::RemainderByZero => {
                Some("run-time error: rest-division by zero".to_owned())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Value {
    Number(f64),
    Boolean(bool),
    Null,
}

impl Value {
    fn number(self) -> f64 {
        match self {
            Self::Number(value) => value,
            Self::Boolean(value) => f64::from(value),
            Self::Null => 0.0,
        }
    }
}

pub(super) fn evaluate(
    expression: &str,
    options: WikidotParserFunctionOptions,
) -> Result<Value, ExpressionError> {
    if expression.len() > MAX_EXPRESSION_BYTES {
        return Err(ExpressionError::Invalid);
    }
    if !expression.is_ascii() {
        return Err(ExpressionError::SyntaxNear(expression.to_owned()));
    }

    let mut parser = ExpressionParser {
        input: expression.as_bytes(),
        offset: 0,
        operations: 0,
        parentheses: 0,
        options,
    };
    let result = match parser.parse_or() {
        Ok(value) => value,
        Err(ExpressionError::Invalid) => {
            let tail = syntax_tail(expression, parser.offset);
            if tail.is_empty() {
                return Err(ExpressionError::Invalid);
            }
            return Err(ExpressionError::SyntaxNear(tail));
        }
        Err(error) => return Err(error),
    };
    parser.skip_space();
    if parser.offset != parser.input.len() {
        let tail = syntax_tail(expression, parser.offset);
        let tail_ref = tail.as_str();
        if tail_ref.starts_with(',') && is_safe_error_tail(tail_ref) {
            return Err(ExpressionError::MisplacedComma);
        }
        if leading_number(tail_ref) {
            return Err(ExpressionError::TooManyValues);
        }
        if let Some(identifier) = leading_identifier(tail_ref) {
            return Err(ExpressionError::UndefinedConstant(identifier.to_owned()));
        }
        return Err(ExpressionError::SyntaxNear(tail));
    }
    if matches!(result, Value::Number(value) if !value.is_finite()) {
        return Err(ExpressionError::Invalid);
    }
    Ok(result)
}

fn syntax_tail(expression: &str, offset: usize) -> String {
    expression[offset..]
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn leading_identifier(tail: &str) -> Option<&str> {
    let tail = tail.trim_start_matches(|character: char| character.is_ascii_whitespace());
    if !tail
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    let end = tail
        .bytes()
        .position(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
        .unwrap_or(tail.len());
    Some(&tail[..end])
}

fn leading_number(tail: &str) -> bool {
    let tail = tail.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let bytes = tail.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_digit)
        || (bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(u8::is_ascii_digit))
}

fn is_safe_error_tail(tail: &str) -> bool {
    !tail.is_empty()
        && tail.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || byte.is_ascii_whitespace()
                || b"_|+-*/%<>=!().,".contains(&byte)
        })
}

#[derive(Debug)]
struct ExpressionParser<'a> {
    input: &'a [u8],
    offset: usize,
    operations: usize,
    parentheses: usize,
    options: WikidotParserFunctionOptions,
}

impl ExpressionParser<'_> {
    fn parse_or(&mut self) -> Result<Value, ExpressionError> {
        let mut value = self.parse_and()?;
        while self.consume("||") {
            self.operation()?;
            let right = self.parse_and()?;
            value = Value::Boolean(truthy(value) || truthy(right));
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<Value, ExpressionError> {
        let mut value = self.parse_comparison()?;
        while self.consume("&&") {
            self.operation()?;
            let right = self.parse_comparison()?;
            value = Value::Boolean(truthy(value) && truthy(right));
        }
        Ok(value)
    }

    fn parse_comparison(&mut self) -> Result<Value, ExpressionError> {
        let left = self.parse_additive()?;
        let operator = [">=", "<=", "==", "!=", ">", "<"]
            .into_iter()
            .find(|operator| self.consume(operator));
        let Some(operator) = operator else {
            return Ok(left);
        };
        self.operation()?;
        let right = match self.parse_comparison() {
            Ok(value) => value,
            Err(ExpressionError::Invalid)
                if operator == "==" && self.remaining().starts_with('=') =>
            {
                return Err(ExpressionError::SyntaxNear(self.remaining().to_owned()));
            }
            Err(ExpressionError::Invalid) => {
                return Err(ExpressionError::TooFewParameters(operator));
            }
            Err(error) => return Err(error),
        };
        let left = left.number();
        let right = right.number();
        let result = match operator {
            ">=" => left >= right,
            "<=" => left <= right,
            "==" => left == right,
            "!=" => left != right,
            ">" => left > right,
            "<" => left < right,
            _ => unreachable!("comparison operator comes from fixed list"),
        };
        Ok(Value::Boolean(result))
    }

    fn parse_additive(&mut self) -> Result<Value, ExpressionError> {
        let mut value = self.parse_multiplicative()?;
        loop {
            if self.consume("+") {
                self.operation()?;
                let right = self.operator_rhs("+", Self::parse_multiplicative)?;
                value = Value::Number(value.number() + right.number());
            } else if self.consume("-") {
                self.operation()?;
                let right = self.operator_rhs("-", Self::parse_multiplicative)?;
                value = Value::Number(value.number() - right.number());
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Value, ExpressionError> {
        let mut value = self.parse_power()?;
        loop {
            if self.consume("*") {
                self.operation()?;
                let right = self.operator_rhs("*", Self::parse_power)?;
                value = Value::Number(value.number() * right.number());
            } else if self.consume("/") {
                self.operation()?;
                let divisor = self.operator_rhs("/", Self::parse_power)?.number();
                if divisor == 0.0 {
                    match self.options.zero_operator_policy {
                        WikidotZeroOperatorPolicy::RuntimeError => {
                            return Err(ExpressionError::DivisionByZero);
                        }
                        WikidotZeroOperatorPolicy::ReplaceOperationWithZero => {
                            value = Value::Number(0.0);
                        }
                    }
                } else {
                    value = Value::Number(value.number() / divisor);
                }
            } else if self.consume("%") {
                self.operation()?;
                let divisor = self.operator_rhs("%", Self::parse_power)?.number().trunc();
                if divisor == 0.0 {
                    match self.options.zero_operator_policy {
                        WikidotZeroOperatorPolicy::RuntimeError => {
                            return Err(ExpressionError::RemainderByZero);
                        }
                        WikidotZeroOperatorPolicy::ReplaceOperationWithZero => {
                            value = Value::Number(0.0);
                        }
                    }
                } else {
                    value = Value::Number(value.number().trunc() % divisor);
                }
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_power(&mut self) -> Result<Value, ExpressionError> {
        let left = self.parse_unary()?;
        if !self.consume("^") {
            return Ok(left);
        }
        self.operation()?;
        let right = self.operator_rhs("^", Self::parse_power)?;
        Ok(Value::Number(left.number().powf(right.number())))
    }

    fn parse_unary(&mut self) -> Result<Value, ExpressionError> {
        if self.consume("+") {
            self.operation()?;
            Ok(Value::Number(self.parse_unary()?.number()))
        } else if self.consume("-") {
            self.operation()?;
            Ok(Value::Number(-self.parse_unary()?.number()))
        } else if self.consume("!") {
            self.operation()?;
            Ok(Value::Number(f64::from(!truthy(self.parse_unary()?))))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Value, ExpressionError> {
        if self.consume("(") {
            self.parentheses += 1;
            if self.parentheses > MAX_PARENTHESES {
                return Err(ExpressionError::Invalid);
            }
            let value = self.parse_or()?;
            if !self.consume(")") {
                return Err(ExpressionError::Invalid);
            }
            self.parentheses -= 1;
            return Ok(value);
        }

        self.skip_space();
        if self
            .input
            .get(self.offset)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return self.parse_function();
        }
        self.parse_number()
    }

    fn parse_function(&mut self) -> Result<Value, ExpressionError> {
        self.skip_space();
        let start = self.offset;
        while self
            .input
            .get(self.offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.offset += 1;
        }
        let name = &self.input[start..self.offset];
        if name == b"true" {
            return Ok(Value::Boolean(true));
        }
        if name == b"false" {
            return Ok(Value::Boolean(false));
        }
        if name.eq_ignore_ascii_case(b"null") {
            return Ok(Value::Null);
        }
        if !self.consume("(") {
            return Err(ExpressionError::UndefinedConstant(
                String::from_utf8_lossy(name).into_owned(),
            ));
        }

        self.parentheses += 1;
        if self.parentheses > MAX_PARENTHESES {
            return Err(ExpressionError::Invalid);
        }
        let mut arguments = vec![self.parse_or()?];
        while self.consume(",") {
            arguments.push(self.parse_or()?);
        }
        if !self.consume(")") {
            return Err(ExpressionError::Invalid);
        }
        self.parentheses -= 1;
        self.operation()?;

        let name_string = String::from_utf8_lossy(name).into_owned();
        match name {
            b"abs" if arguments.len() == 1 => {
                Ok(Value::Number(arguments[0].number().abs()))
            }
            b"abs" if arguments.is_empty() => Err(ExpressionError::TooFewArguments(
                name_string,
                1,
                arguments.len(),
            )),
            b"abs" => Err(ExpressionError::TooManyArguments(
                name_string,
                1,
                arguments.len(),
            )),
            b"min" | b"max" if arguments.len() < 2 => Err(
                ExpressionError::TooFewArguments(name_string, 2, arguments.len()),
            ),
            b"min" => Ok(Value::Number(
                arguments
                    .into_iter()
                    .map(Value::number)
                    .reduce(f64::min)
                    .expect("minimum arity checked"),
            )),
            b"max" => Ok(Value::Number(
                arguments
                    .into_iter()
                    .map(Value::number)
                    .reduce(f64::max)
                    .expect("minimum arity checked"),
            )),
            _ => Err(ExpressionError::UndefinedFunction(name_string)),
        }
    }

    fn parse_number(&mut self) -> Result<Value, ExpressionError> {
        self.skip_space();
        let start = self.offset;
        let mut decimal = false;
        while let Some(byte) = self.input.get(self.offset) {
            if byte.is_ascii_digit() {
                self.offset += 1;
            } else if *byte == b'.' && !decimal {
                decimal = true;
                self.offset += 1;
            } else {
                break;
            }
        }
        if start == self.offset || self.input[start..self.offset] == *b"." {
            return Err(ExpressionError::Invalid);
        }
        std::str::from_utf8(&self.input[start..self.offset])
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .map(Value::Number)
            .ok_or(ExpressionError::Invalid)
    }

    fn consume(&mut self, expected: &str) -> bool {
        self.skip_space();
        if self.input[self.offset..].starts_with(expected.as_bytes()) {
            self.offset += expected.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self
            .input
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }

    fn operator_rhs(
        &mut self,
        operator: &'static str,
        parse: fn(&mut Self) -> Result<Value, ExpressionError>,
    ) -> Result<Value, ExpressionError> {
        match parse(self) {
            Ok(value) => Ok(value),
            Err(ExpressionError::Invalid) => {
                Err(ExpressionError::TooFewParameters(operator))
            }
            Err(error) => Err(error),
        }
    }

    fn remaining(&self) -> &str {
        std::str::from_utf8(&self.input[self.offset..]).unwrap_or_default()
    }

    fn operation(&mut self) -> Result<(), ExpressionError> {
        self.operations += 1;
        if self.operations > MAX_OPERATIONS {
            Err(ExpressionError::Invalid)
        } else {
            Ok(())
        }
    }
}

pub(super) fn truthy(value: Value) -> bool {
    match value {
        Value::Number(value) => value != 0.0,
        Value::Boolean(value) => value,
        Value::Null => false,
    }
}

pub(super) fn format_value(value: Value) -> String {
    match value {
        Value::Boolean(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Number(value) => format_number(value),
    }
}

fn format_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.abs() < 1e-11 {
        let scientific = format!("{value:.1E}");
        let Some((mantissa, exponent)) = scientific.split_once('E') else {
            return scientific;
        };
        let exponent = exponent
            .strip_prefix('+')
            .unwrap_or(exponent)
            .trim_start_matches('0');
        let exponent = if exponent.is_empty() { "0" } else { exponent };
        return format!("{mantissa}E{exponent}");
    }
    let mut output = format!("{value:.12}");
    while output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.pop();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> Result<Value, ExpressionError> {
        Ok(Value::Number(value))
    }

    #[test]
    fn evaluates_precedence_functions_and_boolean_operators() {
        let options = WikidotParserFunctionOptions::default();
        assert_eq!(evaluate("2*(2-1)", options), number(2.0));
        assert_eq!(evaluate("abs(-100)", options), number(100.0));
        assert_eq!(evaluate("min(4,1,-4,6,-10)", options), number(-10.0));
        assert_eq!(evaluate("max(4,1,-4,6,-10)", options), number(6.0));
        assert_eq!(
            evaluate("0 || (1 && !0)", options),
            Ok(Value::Boolean(true))
        );
        assert_eq!(evaluate("1 || 0", options), Ok(Value::Boolean(true)));
        assert_eq!(evaluate("0 && 1", options), Ok(Value::Boolean(false)));
    }

    #[test]
    fn evaluates_ordinary_operator_and_literal_boundaries() {
        let options = WikidotParserFunctionOptions::default();

        for (expression, expected) in [
            ("1<2", Value::Boolean(true)),
            ("8/2", Value::Number(4.0)),
            ("5%2", Value::Number(1.0)),
            ("+2", Value::Number(2.0)),
            ("true + false", Value::Number(1.0)),
            ("1.5 + .5", Value::Number(2.0)),
        ] {
            assert_eq!(evaluate(expression, options), Ok(expected), "{expression}");
        }
        assert_eq!(format_value(Value::Number(0.0)), "0");
    }

    #[test]
    fn rejects_malformed_depth_and_operation_boundaries() {
        let options = WikidotParserFunctionOptions::default();
        let deep_parentheses = format!(
            "{}1{}",
            "(".repeat(MAX_PARENTHESES + 1),
            ")".repeat(MAX_PARENTHESES + 1),
        );
        let deep_functions = format!(
            "{}1{}",
            "abs(".repeat(MAX_PARENTHESES + 1),
            ")".repeat(MAX_PARENTHESES + 1),
        );

        assert_eq!(
            evaluate("1 2", options),
            Err(ExpressionError::TooManyValues),
        );
        for expression in ["(1", "abs(1", "."] {
            assert_eq!(
                evaluate(expression, options),
                Err(ExpressionError::Invalid),
                "{expression}",
            );
        }
        for expression in [deep_parentheses.as_str(), deep_functions.as_str()] {
            assert!(evaluate(expression, options).is_err(), "{expression}");
        }

        let mut parser = ExpressionParser {
            input: b"",
            offset: 0,
            operations: MAX_OPERATIONS,
            parentheses: 0,
            options,
        };
        assert_eq!(parser.operation(), Err(ExpressionError::Invalid));
    }

    #[test]
    fn wikidot_error_fidelity_for_unknown_functions_and_extra_values() {
        let options = WikidotParserFunctionOptions::default();
        assert_eq!(
            evaluate("unknown_func(1)", options),
            Err(ExpressionError::UndefinedFunction(
                "unknown_func".to_owned()
            )),
        );
        assert_eq!(
            evaluate("unknown_func(1)", options)
                .unwrap_err()
                .runtime_message(),
            Some(r#"run-time error: undefined function "unknown_func""#.to_owned()),
        );
        assert_eq!(
            evaluate("abc_def", options),
            Err(ExpressionError::UndefinedConstant("abc_def".to_owned())),
        );
        assert_eq!(
            evaluate("1..2", options),
            Err(ExpressionError::TooManyValues),
        );
        assert_eq!(
            evaluate("1..2", options).unwrap_err().runtime_message(),
            Some("run-time error: too many values in the stack".to_owned()),
        );
    }

    #[test]
    fn distinguishes_zero_operator_errors() {
        let options = WikidotParserFunctionOptions::default();
        assert_eq!(
            evaluate("1/0+1", options),
            Err(ExpressionError::DivisionByZero),
        );
        assert_eq!(
            evaluate("1%0", options),
            Err(ExpressionError::RemainderByZero),
        );
    }

    #[test]
    fn optional_zero_policy_replaces_only_the_zero_operator_result() {
        let options = WikidotParserFunctionOptions {
            zero_operator_policy: WikidotZeroOperatorPolicy::ReplaceOperationWithZero,
        };
        assert_eq!(evaluate("1/0+1", options), number(1.0));
        assert_eq!(evaluate("5%0+2", options), number(2.0));
    }

    #[test]
    fn invalid_and_bounded_expressions_are_distinct_from_runtime_errors() {
        let options = WikidotParserFunctionOptions::default();
        assert_eq!(
            evaluate("abs(1,2)", options),
            Err(ExpressionError::TooManyArguments("abs".to_owned(), 1, 2)),
        );
        assert_eq!(
            evaluate("missing", options),
            Err(ExpressionError::UndefinedConstant("missing".to_owned())),
        );
        assert_eq!(
            evaluate("1 +", options),
            Err(ExpressionError::TooFewParameters("+")),
        );
        assert_eq!(
            evaluate("1,2", options),
            Err(ExpressionError::MisplacedComma),
        );
        assert_eq!(
            evaluate("1|YES|NO", options),
            Err(ExpressionError::SyntaxNear("|YES|NO".to_owned())),
        );
        assert_eq!(
            evaluate("unknown(1)", options),
            Err(ExpressionError::UndefinedFunction("unknown".to_owned())),
        );
        assert_eq!(
            evaluate(&"1+".repeat(129), options),
            Err(ExpressionError::TooFewParameters("+")),
        );
    }
}
