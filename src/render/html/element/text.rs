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
use crate::tree::AttributeMap;

pub fn render_wikitext_raw(ctx: &mut HtmlContext, text: &str) {
    debug!("Escaping raw string '{text}'");

    match ctx.layout() {
        Layout::Wikidot => {
            ctx.html()
                .span()
                .attr(attr!("style" => "white-space: pre-wrap;"))
                .inner(|ctx| ctx.push_escaped_wikidot_raw(text));
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

pub(crate) fn wikidot_obfuscated_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return email.to_owned();
    };
    let reversed_local = local.chars().rev().collect::<String>();
    let reversed_domain = domain.chars().rev().collect::<String>();
    format!("{reversed_domain}|{reversed_local}#{reversed_domain}|{reversed_local}")
}

pub fn render_code(
    ctx: &mut HtmlContext,
    language: Option<&str>,
    contents: &str,
    open_elements: &[Element],
    label_spans: &[AttributeMap],
) {
    debug!(
        "Rendering code block (language {})",
        language.unwrap_or("<none>"),
    );
    let index = ctx.next_code_snippet_index();
    ctx.handle().post_code(index, contents);

    if ctx.layout().legacy() {
        render_wikidot_code(ctx, language, contents, open_elements, label_spans);
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

/// Escapes plain (unhighlighted) code bodies for the Wikidot layout the way
/// live does: `&`, `<`, `>`, and `"` are escaped, while `'` is left as-is.
/// (The shared escaped-content path would render `'` as `&#39;`, which live
/// never emits inside code.)
fn escape_wikidot_code(contents: &str) -> String {
    let mut out = String::with_capacity(contents.len());
    for ch in contents.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn render_wikidot_code(
    ctx: &mut HtmlContext,
    language: Option<&str>,
    contents: &str,
    open_elements: &[Element],
    label_spans: &[AttributeMap],
) {
    ctx.html()
        .div()
        .attr(attr!("class" => "code"))
        .inner(|ctx| {
            render_elements(ctx, open_elements);
            if contents.is_empty() {
                // Live keeps an empty hl-main wrapper (without a <pre>) when
                // the type maps to a highlighter; plain and unmapped code
                // render nothing inside the code div.
                if let Some(language) = language
                    && super::highlight::is_highlighted_language(language)
                {
                    ctx.html()
                        .div()
                        .attr(attr!("class" => "hl-main"))
                        .inner(|ctx| {
                            render_wikidot_code_contents(
                                ctx,
                                label_spans,
                                &mut |_ctx: &mut HtmlContext| {},
                            );
                        });
                }
                return;
            }

            let render_plain = |ctx: &mut HtmlContext| {
                let escaped = escape_wikidot_code(contents);
                ctx.html().pre().inner(|ctx| {
                    render_wikidot_code_contents(ctx, label_spans, &mut |ctx| {
                        ctx.html().code().inner(|ctx| {
                            ctx.push_raw_str(&escaped);
                        });
                    });
                });
            };

            match language {
                Some(language) => {
                    match super::highlight::highlight_code(language, contents) {
                        Some(highlighted) => {
                            ctx.html().div().attr(attr!("class" => "hl-main")).inner(
                                |ctx| {
                                    ctx.html().pre().inner(|ctx| {
                                        render_wikidot_code_contents(
                                            ctx,
                                            label_spans,
                                            &mut |ctx| ctx.push_raw_str(&highlighted),
                                        );
                                    });
                                },
                            );
                        }
                        None => render_plain(ctx),
                    }
                }
                None => render_plain(ctx),
            }
        });
}

fn render_wikidot_code_contents(
    ctx: &mut HtmlContext,
    label_spans: &[AttributeMap],
    contents: &mut dyn FnMut(&mut HtmlContext),
) {
    let Some((attributes, remaining)) = label_spans.split_first() else {
        contents(ctx);
        return;
    };
    ctx.html()
        .span()
        .attr(attr!(;; attributes))
        .inner(|ctx| render_wikidot_code_contents(ctx, remaining, contents));
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
