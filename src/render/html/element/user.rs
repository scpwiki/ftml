/*
 * render/html/element/user.rs
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
use crate::url::normalize_href;
use std::borrow::Cow;

pub fn render_user(ctx: &mut HtmlContext, name: &str, show_avatar: bool) {
    debug!("Rendering user block (name '{name}', show-avatar {show_avatar})");

    match ctx.layout() {
        Layout::Wikidot => render_user_wikidot(ctx, name, show_avatar),
        Layout::Wikijump => render_user_wikijump(ctx, name, show_avatar),
    }
}

pub(super) fn wikidot_user_ends_open_anchor(
    ctx: &HtmlContext,
    element: &Element,
) -> bool {
    let Element::User { name, .. } = element else {
        return false;
    };
    !name.eq_ignore_ascii_case("anonymous") && ctx.user_info(name).is_some()
}

pub(super) fn render_wikidot_user_bare(ctx: &mut HtmlContext, element: &Element) {
    let Element::User { name, show_avatar } = element else {
        return;
    };
    let Some(user_info) = ctx.user_info(name) else {
        return;
    };
    render_wikidot_user_anchors(ctx, &user_info, *show_avatar);
}

fn render_user_wikidot(ctx: &mut HtmlContext, name: &str, show_avatar: bool) {
    if name.eq_ignore_ascii_case("anonymous") {
        ctx.push_escaped("Anonymous");
        return;
    }

    let handle = ctx.handle();

    match ctx.user_info(name) {
        Some(user_info) => {
            let printuser_class = if show_avatar {
                "printuser avatarhover"
            } else {
                "printuser"
            };

            ctx.html()
                .span()
                .attr(attr!("class" => printuser_class))
                .inner(|ctx| {
                    render_wikidot_user_anchors(ctx, &user_info, show_avatar);
                });
        }
        None => {
            let (message_pre, message_post) = {
                let page_info = ctx.info();
                let language = &page_info.language;
                let message_pre = handle.get_message(language, "user-missing-pre");
                let message_post = handle.get_message(language, "user-missing-post");
                (message_pre, message_post)
            };

            ctx.push_escaped(message_pre);

            ctx.html()
                .span()
                .attr(attr!("class" => "error-inline"))
                .inner(|ctx| {
                    ctx.html().em().contents(name);
                    ctx.push_escaped(message_post);
                });
        }
    }
}

fn render_wikidot_user_anchors(
    ctx: &mut HtmlContext,
    user_info: &crate::data::UserInfo<'_>,
    show_avatar: bool,
) {
    let user_profile_url = normalize_href(&user_info.user_profile_url, None);
    let user_avatar_src = normalize_user_avatar_src(&user_info.user_avatar_data);
    let wikidot_onclick = format!(
        "WIKIDOT.page.listeners.userInfo({}); return false;",
        user_info.user_id,
    );
    let wikidot_karma_style = format!(
        "background-image:url({}/userkarma.php?u={})",
        wikidot_avatar_base(&user_info.user_avatar_data)
            .unwrap_or(DEFAULT_WIKIDOT_AVATAR_BASE),
        user_info.user_id,
    );

    if show_avatar {
        ctx.html()
            .a()
            .attr(attr!(
                "href" => &user_profile_url,
                "onclick" => &wikidot_onclick,
            ))
            .inner(|ctx| {
                ctx.html().img().attr(attr!(
                    "class" => "small",
                    "src" => &user_avatar_src,
                    "alt" => &user_info.user_name,
                    "style" => &wikidot_karma_style,
                ));
            });
    }

    ctx.html()
        .a()
        .attr(attr!(
            "href" => &user_profile_url,
            "onclick" => &wikidot_onclick,
        ))
        .contents(&user_info.user_name);
}

fn render_user_wikijump(ctx: &mut HtmlContext, name: &str, show_avatar: bool) {
    ctx.html()
        .span()
        .attr(attr!("class" => "wj-user-info"))
        .inner(|ctx| match ctx.user_info(name) {
            Some(info) => {
                let user_profile_url = normalize_href(&info.user_profile_url, None);
                let user_avatar_src = normalize_user_avatar_src(&info.user_avatar_data);
                trace!(
                    "Got user information (user id {}, name {})",
                    info.user_id,
                    info.user_name.as_ref(),
                );

                ctx.html()
                    .a()
                    .attr(attr!(
                        "class" => "wj-user-info-link",
                        "href" => &user_profile_url,
                    ))
                    .inner(|ctx| {
                        if show_avatar {
                            ctx.html()
                                .span()
                                .attr(attr!(
                                    "class" => "wj-karma",
                                    "data-karma" => &info.user_karma.to_string(),
                                ))
                                .inner(|ctx| {
                                    ctx.html().sprite("wj-karma");
                                });

                            ctx.html().img().attr(attr!(
                                "class" => "wj-user-info-avatar",
                                "src" => &user_avatar_src,
                            ));
                        }

                        ctx.html()
                            .span()
                            .attr(attr!("class" => "wj-user-info-name"))
                            .contents(&info.user_name);
                    });
            }
            None => {
                trace!("No such user found");

                ctx.html()
                    .span()
                    .attr(attr!("class" => "wj-error-inline"))
                    .inner(|ctx| {
                        if show_avatar {
                            ctx.html()
                                .span()
                                .attr(attr!(
                                    "class" => "wj-karma",
                                    "data-karma" => "0",
                                ))
                                .inner(|ctx| {
                                    ctx.html().sprite("wj-karma");
                                });

                            ctx.html().img().attr(attr!(
                                "class" => "wj-user-info-avatar",
                                "src" => "/files--static/media/bad-avatar.png",
                            ));
                        }

                        ctx.html()
                            .span()
                            .attr(attr!("class" => "wj-user-info-name"))
                            .contents(name);
                    });
            }
        });
}

const DEFAULT_WIKIDOT_AVATAR_BASE: &str = "http://www.wikidot.com";

fn wikidot_avatar_base(avatar_src: &str) -> Option<&str> {
    let scheme_len = ["https://", "http://"]
        .iter()
        .find(|scheme| avatar_src.starts_with(**scheme))?
        .len();
    let host_len = avatar_src[scheme_len..]
        .find('/')
        .unwrap_or(avatar_src.len() - scheme_len);
    let host = &avatar_src[scheme_len..scheme_len + host_len];
    let base = &avatar_src[..scheme_len + host_len];
    let valid_host = !host.is_empty()
        && host.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':')
        });
    (valid_host && avatar_src[base.len()..].starts_with("/avatar.php?")).then_some(base)
}

fn normalize_user_avatar_src(src: &str) -> Cow<'_, str> {
    if is_safe_image_data_uri(src) {
        Cow::Borrowed(src)
    } else {
        normalize_href(src, None)
    }
}

fn is_safe_image_data_uri(src: &str) -> bool {
    const SAFE_DATA_PREFIXES: &[&str] = &[
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/jpg;base64,",
        "data:image/gif;base64,",
        "data:image/webp;base64,",
        "data:image/bmp;base64,",
        "data:image/x-icon;base64,",
        "data:image/vnd.microsoft.icon;base64,",
    ];

    let Some(prefix) = SAFE_DATA_PREFIXES.iter().find(|prefix| {
        src.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    }) else {
        return false;
    };

    let data = &src[prefix.len()..];
    !data.is_empty()
        && data.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'+' | b'/' | b'=' | b'\r' | b'\n')
        })
}

#[cfg(test)]
mod tests {
    use super::{is_safe_image_data_uri, normalize_user_avatar_src, wikidot_avatar_base};
    use crate::url::normalize_href;

    #[test]
    fn user_supplied_profile_and_avatar_urls_are_normalized() {
        assert_eq!(
            normalize_href("javascript:alert(1)", None).as_ref(),
            "#invalid-url",
        );
        assert_eq!(
            normalize_user_avatar_src("data:text/html,<script>alert(1)</script>")
                .as_ref(),
            "#invalid-url",
        );
        assert_eq!(
            normalize_user_avatar_src("data:image/svg+xml,<svg onload=alert(1)>")
                .as_ref(),
            "#invalid-url",
        );
        assert_eq!(
            normalize_user_avatar_src("data:image/png;base64,aGVsbG8=").as_ref(),
            "data:image/png;base64,aGVsbG8=",
        );

        assert!(is_safe_image_data_uri(
            "data:image/webp;base64,AAAA\r\nBBBB=="
        ));
        assert!(!is_safe_image_data_uri("data:image/png;base64,<>"));
    }

    #[test]
    fn wikidot_avatar_base_requires_a_wikidot_avatar_url() {
        assert_eq!(
            wikidot_avatar_base(
                "https://www.wikidot.com/avatar.php?userid=1&amp;size=small"
            ),
            Some("https://www.wikidot.com"),
        );
        assert_eq!(
            wikidot_avatar_base("http://www.wikidot.com/avatar.php?userid=1"),
            Some("http://www.wikidot.com"),
        );
        assert_eq!(wikidot_avatar_base("data:image/png;base64,AAAA"), None);
        assert_eq!(
            wikidot_avatar_base("https://www.wikidot.com/userkarma.php?u=1"),
            None,
        );
        assert_eq!(
            wikidot_avatar_base("https://evil.example)/avatar.php?userid=1"),
            None,
        );
        assert_eq!(wikidot_avatar_base("https:///avatar.php?userid=1"), None);
    }
}
