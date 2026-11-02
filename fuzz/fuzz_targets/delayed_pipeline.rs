#![no_main]

use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    ResolvedTagRef, SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use libfuzzer_sys::fuzz_target;
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("fuzz"),
        category: None,
        site: Cow::Borrowed("fuzz"),
        title: Cow::Borrowed("Fuzz"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn generated_value(kind: GeneratedKind) -> GeneratedValue<'static> {
    match kind {
        GeneratedKind::PageLink => GeneratedValue::PageLink {
            page: PageRef::page_only("fuzz-target"),
            label: Cow::Borrowed("Fuzz Target"),
        },
        GeneratedKind::TagLinks => GeneratedValue::TagLinks {
            tags: Cow::Owned(vec![ResolvedTagRef {
                tag: Cow::Borrowed("fuzz"),
            }]),
            separator: Cow::Borrowed(", "),
        },
    }
}

fn text_origin(value: u8) -> TextOrigin {
    match value % 3 {
        0 => TextOrigin::Authored,
        1 => TextOrigin::RuntimeScalar,
        _ => TextOrigin::RuntimeLiteral,
    }
}

fn generated_kind(value: u8) -> GeneratedKind {
    if value & 1 == 0 {
        GeneratedKind::PageLink
    } else {
        GeneratedKind::TagLinks
    }
}

fn character_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut starts = source
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    starts.push(source.len());
    starts
        .windows(2)
        .map(|window| (window[0], window[1]))
        .collect()
}

fn one_generated_input<'a>(
    source: &'a str,
    controls: &[u8; 4],
) -> Option<(DelayedInput<'a>, SlotBindings<'static>)> {
    let ranges = character_ranges(source);
    if ranges.is_empty() {
        return None;
    }
    let &(start, end) = ranges.get(usize::from(controls[1]) % ranges.len())?;
    let kind = generated_kind(controls[2]);
    let segments = vec![
        InputSegment::text(0..start, text_origin(controls[3])),
        InputSegment::generated(GeneratedInput {
            source_range: start..end,
            id: SlotId::new(1),
            kind,
            occurrence: 0,
        }),
        InputSegment::text(end..source.len(), text_origin(controls[0])),
    ];
    let input = DelayedInput::new(source, segments).ok()?;
    let bindings =
        SlotBindings::new(vec![(SlotId::new(1), generated_value(kind))]).ok()?;
    Some((input, bindings))
}

fn two_generated_input<'a>(
    source: &'a str,
    controls: &[u8; 4],
    repeated_slot: bool,
) -> Option<(DelayedInput<'a>, SlotBindings<'static>)> {
    let ranges = character_ranges(source);
    if ranges.len() < 2 {
        return None;
    }
    let first_index = usize::from(controls[1]) % (ranges.len() - 1);
    let second_index =
        first_index + 1 + usize::from(controls[2]) % (ranges.len() - first_index - 1);
    let (first_start, first_end) = ranges[first_index];
    let (second_start, second_end) = ranges[second_index];
    let first_kind = generated_kind(controls[0]);
    let second_kind = if repeated_slot {
        first_kind
    } else {
        generated_kind(controls[3])
    };
    let second_id = if repeated_slot {
        SlotId::new(1)
    } else {
        SlotId::new(2)
    };
    let segments = vec![
        InputSegment::text(0..first_start, text_origin(controls[3])),
        InputSegment::generated(GeneratedInput {
            source_range: first_start..first_end,
            id: SlotId::new(1),
            kind: first_kind,
            occurrence: 0,
        }),
        InputSegment::text(first_end..second_start, text_origin(controls[1])),
        InputSegment::generated(GeneratedInput {
            source_range: second_start..second_end,
            id: second_id,
            kind: second_kind,
            occurrence: if repeated_slot { 1 } else { 0 },
        }),
        InputSegment::text(second_end..source.len(), text_origin(controls[2])),
    ];
    let input = DelayedInput::new(source, segments).ok()?;
    let mut values = vec![(SlotId::new(1), generated_value(first_kind))];
    if !repeated_slot {
        values.push((SlotId::new(2), generated_value(second_kind)));
    }
    let bindings = SlotBindings::new(values).ok()?;
    Some((input, bindings))
}

fn authored_input<'a>(
    source: &'a str,
    origin: TextOrigin,
) -> Option<(DelayedInput<'a>, SlotBindings<'static>)> {
    let input =
        DelayedInput::new(source, vec![InputSegment::text(0..source.len(), origin)])
            .ok()?;
    Some((input, SlotBindings::empty()))
}

fn exercise(
    input: &DelayedInput<'_>,
    bindings: &SlotBindings<'_>,
    layout: Layout,
    inline: bool,
) {
    let page_info = page_info();
    let mut settings = WikitextSettings::from_mode(WikitextMode::List, layout);
    settings.list_pages_inline = inline;
    let Ok(delayed) = parse_delayed_list(input, &page_info, &settings) else {
        return;
    };
    let Ok(bound) = delayed.bind(bindings) else {
        return;
    };
    std::hint::black_box(bound.render_html(&page_info, &settings));
}

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    if data.is_empty() {
        return;
    }
    let c0 = data.len() as u8;
    let c1 = data.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
    let c2 = data.iter().fold(0_u8, |xor, byte| xor ^ *byte);
    let c3 = data[data.len() / 2];
    let controls = [c0, c1, c2, c3];

    let candidate = match c0 % 6 {
        0 => authored_input(source, TextOrigin::Authored),
        1 => authored_input(source, TextOrigin::RuntimeScalar),
        2 => authored_input(source, TextOrigin::RuntimeLiteral),
        3 => one_generated_input(source, &controls),
        4 => two_generated_input(source, &controls, false),
        _ => two_generated_input(source, &controls, true),
    };
    let Some((input, bindings)) = candidate else {
        return;
    };

    for layout in [Layout::Wikidot, Layout::Wikijump] {
        exercise(&input, &bindings, layout, false);
        exercise(&input, &bindings, layout, true);
    }
});
