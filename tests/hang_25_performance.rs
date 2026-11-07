use ftml::data::{PageInfo, ScoreValue};
use ftml::includes::DebugIncluder;
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

const HANG_25_PREFIX: &str = r#",, @@a//**é[!--
__a__ [[span]]a[[[[[span]]
[[span]][[[a@@
--a@< @@ @<é**a__//a@@
//**a@<
@@__ [!--é##red|é@@ __é-- --a[[span]]
** {{ {{a[[[ {{ @<a--
//
,,[[span]],,
^^é@@ @@é##red|a[!--a{{é@<é[!-- @<é__
[!--
^^a@@##red|a**
** **
^^é@@a[!--
@< ##red|
[[[a{{
**a[[span]]
@@a^^
[[span]]a[[span]]^^{{**a**a__é,,é##red|__
[[span]]é@<a//é__é[[[a[[span]] [[[é^^ [[span]]é{{
[[[é[[[@@é,, [[[
[!--
##red| **é[[span]]{{ [!--a@@é@@//^^
**é^^a,,é--é,, @@
,, ##red|a,, @@
//
##red| @<é[!-- // [[[ ---- //é[[span]]é[[span]] ,,^^ @@é**
##red|a__é__,,@< @@[[[a,,
@@a--é[!--é__a{{é-- [[span]]é**é[[span]]a@<a[[span]]@<a##red|[[[
--é,, -- ^^a##red|é--
[[[ [[[ ^^ __a{{a##red|a__
[[span]]é[!--é^^,,^^é@@ [[span]]é@@
@@a[!--**é__ [!--é,,
-- ##red| __[!--a,,
[[span]] [[span]]**a[[span]]a##red|a** __[[span]] **[!--a__a[!--
**a__a// __a[[[,,é**
**
##red|
**[!--é--é[!--é{{@@a[!-- -- //,,^^ [!-- **é--
,,a//{{a@<a@< [[[ [[[ //a^^a__a-- -- ** {{
{{a@<
// --é@<^^@<é//a##red|é**^^ ##red| __ ,,a,,
{{a__é**a##red|
^^
[!--
^^é@@
//,,é**é,, --é--a@@ __ @@é{{__a[[[a[!--a__
**é//a^^__é--é**a//a-- {{é@@a##red|a@<é,, ,,
__a[!-- [[[a//é##red|
[!--a[!--__
@@
@@
@@é-- -- ,,é[[span]] [[[é**é--
,,é[[[
{{
**a##red|é[!--##red|
** [[[a@<a@@é@<[!-- __^^a@<é^^a[[[
@<-- @@a##red| //é@<é[!--^^a[[[a**
__
,,
##red|// @@a[!--a--**a__é^^
//
--
,,[[span]]é[[span]]//[!-- ^^ // -- [!--é[[span]] __@@@<
@@{{a{{a**
[[[é[[span]]a[[[[[span]] {{ {{[[[é@@a__é##red|é[[span]] --
//^^a@@é@@
[!--a##red|
,,a{{ @<a@@
^^é##red|é[[[[[span]]a[!--a// [[span]]
** ^^__a--é,,a-- ##red|
,,a@@a@< __@@ {{ ##red| [[span]]é##red|a__^^ --
^^a^^
**a** ^^
^^ [!-- [[span]]a^^[!-- //{{a{{
**é[[[[!--a[[span]]
[[span]] ,,a__ ##red|a-- --
,, @@ ##red|a##red|é[!--a[!--
__ [!-- @<a[[span]] @<
[!--é//
{{
[[span]]
[[span]] @@é@@é__é@@
--
//
** ^^
{{é[!--
^^é@<é__
@<é@<
^^a@<a__é--a,, __é@<a[!--a^^ {{@<
[!-- {{a[[span]]
@< [!-- ^^ ^^é^^
[!--
##red|
//é//é[!-- ** ,,
__--//a__^^ {{a[!--
^^
--é__ [[[**
-- [!--
----^^a@<
[[span]]é@@
@@ __
[[span]]é// @@
,,@@##red| @@
^^é// {{ // ** @<é@@a[!--,, __é[[span]]@<é[!--[!--a-- ^^
__a**a,,//é-- @@@@é[[[a{{
[[span]]
[[span]] @<^^^^
--a@<é[[[ --
,,a**
[[span]] ##red| **
@<[[span]]
@<é[[[
--é@<é__##red| @<[[[[[span]]--é@@a^^a--
--
//a__
{{@@a--a// ,,a@<-- __ [[span]]a,,é##red|a##red|,, **
,,é[!--//a^^é-- ^^ {{ [[[ @<a##red|
{{a--é@@é@@é@@ [[span]]a@@
--é**^^[[span]]##red|é[[[ ##red|a[[span]]é,,{{a{{ ^^a--##red|é--__##red|é__é@@a{{[!--a__// {{##red|**a__é-- @< ^^a^^ ^^a** __
[[[
[[span]]é[[span]]a@@é//
,,a//@<é__ [[span]] **é##red| [[[[!--[!--##red| **//__é{{{{@@[[span]]^^ [[[//@@
[[span]] [!--a[[span]] ^^
--a__[!--@@a@<
^^a[[span]]é^^é@@a@@ __@@
@< @@ @<
**é**a[[[
@<a{{^^a##red|[[span]]a^^a##red|a@<
@< @@
[[[
[[[//
__a-- @<é^^
[[[ ____
@@a////é{{ [!--//a##red|
[[span]] __a// ,,é[[[é[!--é@< {{[[[__a[!--{{ {{é@@ @< //a{{^^@<a-- [[span]]a__é//
{{é@@
__
@<a[[[a@@é,,a^^ [[[é{{^^a--@@ {{^^é//
@@##red|é@@a@<
// ^^
** {{é//##red|@@a**é//__ {{é{{a@< [[[[[span]] [[span]][[[a--é^^^^@<[[[
,,
^^é--
** ##red|é**a//[!--a[[[
^^##red| @<
[[span]]
##red|é,,é,,** **
##red|,, //a[!--{{ --{{** [[[[[[a**a##red| @@{{a##red|a^^[!--
--é{{,, -- --
@@
//a{{[!-- [[[é[!--
[!--é[!--##red|[!--** @@
-- @<[[span]]^^é##red|é@@a{{ ** //##red| ,,[[span]]
--__a##red|
{{
[!-- __^^ ##red|^^//
^^
##red|é##red|
[[span]] // @< ^^ {{é@<a[[span]]a[[span]]--a##red|a##red|
[[span]]
^^__ __a^^##red| ^^[!-- [[[
**
** @@a##red|a@<,,##red|é[!--
@@a[!-- ////é{{é__a^^
-- ////
^^ **[[[ __ @@é//é{{//
[[span]] --a[[span]]é[!--
##red|a// [[span]]é@@é@@ [!--[[[ [[[
[!--a@< {{
^^a##red|
[[span]]//[[span]]
-- @< //é[!--a,,a__é[[[ ,,é**^^ @<
__a@<é{{a##red|é__ **é@@[!--a##red|a__
[[[[[span]]
**
"#;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("fuzz-page"),
        category: Some(Cow::Borrowed("fuzz")),
        site: Cow::Borrowed("fuzz"),
        title: Cow::Borrowed("Fuzz"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("a"), Cow::Borrowed("_b")],
        language: Cow::Borrowed("en"),
    }
}

#[test]
fn hang_25_prefix_parses_within_budget() {
    let page_info = page_info();
    let mut settings = WikitextSettings::from_mode(WikitextMode::Draft, Layout::Wikijump);
    settings.enable_html_blocks = false;
    settings.use_true_ids = true;
    settings.isolate_user_ids = true;

    let started = Instant::now();
    let mut text = ftml::include(
        HANG_25_PREFIX,
        &settings,
        DebugIncluder::all_present(),
        || unreachable!(),
    )
    .unwrap()
    .0;
    ftml::preprocess(&mut text);
    let tokenization = ftml::tokenize(&text);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let elapsed = started.elapsed();

    assert!(!tree.elements.is_empty());
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}
