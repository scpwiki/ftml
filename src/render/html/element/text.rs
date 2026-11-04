/*
 * render/html/element/text.rs
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

use super::prelude::*;

pub fn render_wikitext_raw(ctx: &mut HtmlContext, text: &str) {
    debug!("Escaping raw string '{text}'");

    match ctx.layout() {
        Layout::Wikidot => {
            ctx.html()
                .span()
                .attr(attr!("style" => "white-space: pre-wrap;"))
                .contents(text);
        }
        Layout::Wikijump => {
            ctx.html()
                .span()
                .attr(attr!("class" => "wj-raw"))
                .contents(text);
        }
    }
}

pub fn render_email(ctx: &mut HtmlContext, email: &str) {
    debug!("Rendering email address '{email}'");

    match ctx.layout() {
        Layout::Wikidot => {
            let obfuscated = wikidot_obfuscated_email(email);
            ctx.html()
                .span()
                .attr(attr!("class" => "wiki-email"))
                .contents(&obfuscated);
        }
        Layout::Wikijump => {
            ctx.html()
                .span()
                .attr(attr!("class" => "wj-email"))
                .contents(email);
        }
    }
}

fn wikidot_obfuscated_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return email.to_owned();
    };
    let reversed_local = local.chars().rev().collect::<String>();
    let reversed_domain = domain.chars().rev().collect::<String>();
    format!("{reversed_domain}|{reversed_local}#{reversed_domain}|{reversed_local}")
}

pub fn render_code(ctx: &mut HtmlContext, language: Option<&str>, contents: &str) {
    debug!(
        "Rendering code block (language {})",
        language.unwrap_or("<none>"),
    );
    let index = ctx.next_code_snippet_index();
    ctx.handle().post_code(index, contents);

    if ctx.layout().legacy() {
        render_wikidot_code(ctx, language, contents);
        return;
    }

    let class = {
        let mut class = format!("wj-code wj-language-{}", language.unwrap_or("none"));
        class.make_ascii_lowercase();
        class
    };

    ctx.html()
        .element("wj-code")
        .attr(attr!("class" => &class))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "wj-code-panel",
                ))
                .inner(|ctx| {
                    let button_title = ctx
                        .handle()
                        .get_message(ctx.language(), "button-copy-clipboard");

                    ctx.html()
                        .element("wj-code-copy")
                        .attr(attr!(
                            "type" => "button",
                            "class" => "wj-code-copy",
                            "title" => button_title,
                        ))
                        .inner(|ctx| {
                            ctx.html().sprite("wj-clipboard");
                            ctx.html().sprite("wj-clipboard-success");
                        });

                    ctx.html()
                        .span()
                        .attr(attr!(
                            "class" => "wj-code-language",
                        ))
                        .contents(language.unwrap_or(""));
                });

            ctx.html().pre().inner(|ctx| {
                ctx.html().code().contents(contents);
            });
        });
}

fn render_wikidot_code(ctx: &mut HtmlContext, language: Option<&str>, contents: &str) {
    ctx.html()
        .div()
        .attr(attr!("class" => "code"))
        .inner(|ctx| {
            if contents.is_empty() {
                return;
            }

            match language {
                Some("css" | "python" | "java") => {
                    ctx.html()
                        .div()
                        .attr(attr!("class" => "hl-main"))
                        .inner(|ctx| {
                            ctx.html().pre().inner(|ctx| {
                                render_wikidot_highlight(
                                    ctx,
                                    language.unwrap(),
                                    contents,
                                );
                            });
                        });
                }
                _ => {
                    ctx.html().pre().inner(|ctx| {
                        ctx.html().code().contents(contents);
                    });
                }
            }
        });
}

fn render_wikidot_highlight(ctx: &mut HtmlContext, language: &str, contents: &str) {
    let mut rest = contents;
    while !rest.is_empty() {
        let (class, length) = wikidot_highlight_token(language, rest);
        let (token, next) = rest.split_at(length);
        ctx.html()
            .span()
            .attr(attr!("class" => class))
            .contents(token);
        rest = next;
    }
}

fn wikidot_highlight_token(language: &str, input: &str) -> (&'static str, usize) {
    let first = input.as_bytes()[0];
    if first.is_ascii_whitespace() || first == b';' {
        let mut length = input
            .bytes()
            .take_while(|byte| byte.is_ascii_whitespace() || *byte == b';')
            .count();
        if language == "css" && input[length..].starts_with("--") {
            length += 2;
        }
        return ("hl-code", length);
    }
    if language == "css" && input.starts_with("--") {
        return ("hl-code", 2);
    }
    if matches!(first, b'{' | b'}') {
        let length = input
            .bytes()
            .take_while(|byte| matches!(byte, b'{' | b'}'))
            .count();
        return ("hl-brackets", length);
    }
    if language == "css" && first == b':' {
        let length = input
            .bytes()
            .take_while(|byte| {
                !byte.is_ascii_whitespace() && !matches!(byte, b'{' | b'}' | b';')
            })
            .count();
        return ("hl-special", length);
    }
    if language == "css" && first.is_ascii_digit() {
        let length = input
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
            .count();
        return ("hl-number", length);
    }
    if language == "css" && first == b'%' {
        return ("hl-string", 1);
    }

    let length = input
        .bytes()
        .take_while(|byte| {
            !byte.is_ascii_whitespace() && !matches!(byte, b'{' | b'}' | b';')
        })
        .count();
    let token = &input[..length];
    if language == "css"
        && let Some(split) = css_pseudo_split(token)
    {
        return ("hl-identifier", split);
    }
    let class = match language {
        "css" if token.ends_with(':') => "hl-reserved",
        "css" if token == "none" => "hl-string",
        "python" if token == "import" => "hl-reserved",
        "java" if matches!(token, "public" | "abstract" | "interface" | "class") => {
            "hl-reserved"
        }
        "java" if matches!(token, "static" | "final") => "hl-types",
        _ => "hl-identifier",
    };
    (class, length)
}

fn css_pseudo_split(token: &str) -> Option<usize> {
    let split = token.bytes().position(|byte| byte == b':')?;
    if split == 0
        || token.ends_with(':')
        || token.as_bytes()[split + 1] == b'/'
        || token[..split].contains(['"', '\''])
    {
        return None;
    }
    Some(split)
}

#[cfg(test)]
mod tests {
    use super::wikidot_obfuscated_email;

    #[test]
    fn wikidot_email_obfuscation_reverses_local_and_domain_independently() {
        assert_eq!(
            wikidot_obfuscated_email("support@wikidot.com"),
            "moc.todikiw|troppus#moc.todikiw|troppus",
        );
        assert_eq!(wikidot_obfuscated_email("not-an-email"), "not-an-email");
    }
}
