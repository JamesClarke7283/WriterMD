use leptos::ev;
use leptos::prelude::*;
use pulldown_cmark::{Options, Parser, html};
use std::collections::HashSet;
use wasm_bindgen::JsCast;

#[derive(Clone, Debug, PartialEq)]
pub struct TextBlock {
    pub id: usize,
    pub text: String,
}

#[derive(Clone, Copy)]
pub struct EditorState {
    pub blocks: RwSignal<Vec<TextBlock>>,
    pub next_id: RwSignal<usize>,
    pub raw_lines: RwSignal<HashSet<usize>>,
    pub active_raw_line: RwSignal<Option<usize>>,
}

impl EditorState {
    pub fn new(content: &str) -> Self {
        let blocks = parse_blocks(content);
        let next_id = blocks.len();
        Self {
            blocks: RwSignal::new(blocks),
            next_id: RwSignal::new(next_id),
            raw_lines: RwSignal::new(HashSet::new()),
            active_raw_line: RwSignal::new(None),
        }
    }

    pub fn sync_from_content(&self, content: &str) {
        self.blocks.set(parse_blocks(content));
        self.next_id.set(self.blocks.get_untracked().len());
        self.raw_lines.set(HashSet::new());
        self.active_raw_line.set(None);
    }

    pub fn to_string(&self) -> String {
        self.blocks
            .get()
            .into_iter()
            .map(|block| block.text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LineKind {
    Blank,
    Paragraph,
    Heading(u8),
    BulletItem,
    OrderedItem,
    TaskItem { checked: bool },
    QuoteLine,
    Hr,
    FenceLine,
    CodeFenceBody,
    RawOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LinePresentation {
    prefix_len: usize,
    visible_text: String,
    marker_text: Option<String>,
    row_classes: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineRenderInfo {
    kind: LineKind,
    presentation: LinePresentation,
    hidden_prefix: bool,
    raw_only: bool,
    auto_raw_on_focus: bool,
}

fn parse_blocks(content: &str) -> Vec<TextBlock> {
    let mut blocks = content
        .split('\n')
        .enumerate()
        .map(|(i, line)| TextBlock {
            id: i,
            text: line.trim_end_matches('\r').replace('\u{200b}', ""),
        })
        .collect::<Vec<_>>();

    if blocks.is_empty() {
        blocks.push(TextBlock {
            id: 0,
            text: String::new(),
        });
    }

    blocks
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn current_block_text(blocks: &[TextBlock], id: usize) -> String {
    blocks
        .iter()
        .find(|block| block.id == id)
        .map(|block| block.text.clone())
        .unwrap_or_default()
}

fn is_fence_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn leading_indent_len(text: &str) -> usize {
    text.char_indices()
        .find_map(|(idx, ch)| (!matches!(ch, ' ' | '\t')).then_some(idx))
        .unwrap_or(text.len())
}

fn heading_prefix(text: &str) -> Option<(u8, usize)> {
    let indent_len = leading_indent_len(text);
    let rest = &text[indent_len..];
    let hashes = rest.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
        Some((hashes as u8, indent_len + hashes + 1))
    } else {
        None
    }
}

fn task_prefix(text: &str) -> Option<(bool, usize)> {
    let indent_len = leading_indent_len(text);
    let rest = &text[indent_len..];
    for (checked, bullet) in [
        (false, "- [ ] "),
        (true, "- [x] "),
        (true, "- [X] "),
        (false, "* [ ] "),
        (true, "* [x] "),
        (true, "* [X] "),
        (false, "+ [ ] "),
        (true, "+ [x] "),
        (true, "+ [X] "),
    ] {
        if let Some(stripped) = rest.strip_prefix(bullet) {
            return Some((checked, text.len() - stripped.len()));
        }
    }
    None
}

fn bullet_prefix(text: &str) -> Option<usize> {
    let indent_len = leading_indent_len(text);
    let rest = &text[indent_len..];
    for bullet in ["- ", "* ", "+ "] {
        if let Some(stripped) = rest.strip_prefix(bullet) {
            return Some(text.len() - stripped.len());
        }
    }
    None
}

fn ordered_prefix(text: &str) -> Option<(String, usize)> {
    let indent_len = leading_indent_len(text);
    let rest = &text[indent_len..];
    let digit_count = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }

    let digits = &rest[..digit_count];
    let marker = &rest[digit_count..];
    if marker.starts_with(". ") {
        return Some((format!("{digits}."), indent_len + digit_count + 2));
    }
    if marker.starts_with(") ") {
        return Some((format!("{digits})"), indent_len + digit_count + 2));
    }
    None
}

fn quote_prefix(text: &str) -> Option<usize> {
    let indent_len = leading_indent_len(text);
    let rest = &text[indent_len..];
    rest.strip_prefix("> ")
        .map(|stripped| text.len() - stripped.len())
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let compact = trimmed.replace(' ', "");
    if compact.len() < 3 {
        return false;
    }

    compact.chars().all(|ch| ch == '-')
        || compact.chars().all(|ch| ch == '*')
        || compact.chars().all(|ch| ch == '_')
}

fn looks_like_table_row(trimmed: &str) -> bool {
    trimmed.contains('|') && (trimmed.starts_with('|') || trimmed.ends_with('|'))
}

fn looks_like_html_block(trimmed: &str) -> bool {
    trimmed.starts_with('<') && trimmed.ends_with('>')
}

fn looks_like_reference_definition(trimmed: &str) -> bool {
    trimmed.starts_with('[') && trimmed.contains("]:")
}

fn row_classes_for(kind: &LineKind, marker_text: Option<&str>, hidden_prefix: bool) -> String {
    let mut classes = vec!["editor-line-shell".to_string()];

    match kind {
        LineKind::Blank => classes.push("is-blank".to_string()),
        LineKind::Paragraph => classes.push("is-paragraph".to_string()),
        LineKind::Heading(level) => {
            classes.push("is-heading".to_string());
            classes.push(format!("is-heading-{level}"));
        }
        LineKind::BulletItem => {
            classes.push("is-list".to_string());
            classes.push("is-bullet".to_string());
        }
        LineKind::OrderedItem => {
            classes.push("is-list".to_string());
            classes.push("is-ordered".to_string());
        }
        LineKind::TaskItem { .. } => {
            classes.push("is-list".to_string());
            classes.push("is-task".to_string());
        }
        LineKind::QuoteLine => {
            classes.push("is-quote".to_string());
            classes.push("has-rail".to_string());
        }
        LineKind::Hr => classes.push("is-hr".to_string()),
        LineKind::FenceLine => classes.push("is-fence-line".to_string()),
        LineKind::CodeFenceBody => classes.push("is-code-fence-body".to_string()),
        LineKind::RawOnly => classes.push("is-raw-fallback".to_string()),
    }

    if hidden_prefix {
        classes.push("has-hidden-prefix".to_string());
    }
    if marker_text.is_some() {
        classes.push("has-marker".to_string());
    }

    classes.join(" ")
}

fn make_info(
    kind: LineKind,
    text: &str,
    prefix_len: usize,
    marker_text: Option<String>,
    hidden_prefix: bool,
    raw_only: bool,
    auto_raw_on_focus: bool,
) -> LineRenderInfo {
    let visible_text = if prefix_len > 0 {
        text[prefix_len..].to_string()
    } else {
        text.to_string()
    };

    let row_classes = row_classes_for(&kind, marker_text.as_deref(), hidden_prefix);

    LineRenderInfo {
        kind,
        presentation: LinePresentation {
            prefix_len,
            visible_text,
            marker_text,
            row_classes,
        },
        hidden_prefix,
        raw_only,
        auto_raw_on_focus,
    }
}

fn analyze_line(text: &str, inside_code_fence: bool) -> LineRenderInfo {
    if inside_code_fence {
        if is_fence_line(text) {
            return make_info(LineKind::FenceLine, text, 0, None, false, true, false);
        }
        return make_info(LineKind::CodeFenceBody, text, 0, None, false, true, false);
    }

    if is_fence_line(text) {
        return make_info(LineKind::FenceLine, text, 0, None, false, true, false);
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return make_info(LineKind::Blank, text, 0, None, false, false, false);
    }

    if looks_like_table_row(trimmed)
        || looks_like_html_block(trimmed)
        || looks_like_reference_definition(trimmed)
    {
        return make_info(LineKind::RawOnly, text, 0, None, false, true, false);
    }

    if is_horizontal_rule(trimmed) {
        return make_info(LineKind::Hr, text, text.len(), None, true, false, true);
    }

    if let Some((level, prefix_len)) = heading_prefix(text) {
        return make_info(
            LineKind::Heading(level),
            text,
            prefix_len,
            None,
            true,
            false,
            false,
        );
    }

    if let Some((checked, prefix_len)) = task_prefix(text) {
        return make_info(
            LineKind::TaskItem { checked },
            text,
            prefix_len,
            Some(if checked {
                "☑".to_string()
            } else {
                "☐".to_string()
            }),
            true,
            false,
            false,
        );
    }

    if let Some(prefix_len) = bullet_prefix(text) {
        return make_info(
            LineKind::BulletItem,
            text,
            prefix_len,
            Some("•".to_string()),
            true,
            false,
            false,
        );
    }

    if let Some((marker_text, prefix_len)) = ordered_prefix(text) {
        return make_info(
            LineKind::OrderedItem,
            text,
            prefix_len,
            Some(marker_text),
            true,
            false,
            false,
        );
    }

    if let Some(prefix_len) = quote_prefix(text) {
        return make_info(
            LineKind::QuoteLine,
            text,
            prefix_len,
            None,
            true,
            false,
            false,
        );
    }

    make_info(LineKind::Paragraph, text, 0, None, false, false, false)
}

fn line_render_info(blocks: &[TextBlock], id: usize) -> LineRenderInfo {
    let mut inside_code_fence = false;

    for block in blocks {
        let info = analyze_line(&block.text, inside_code_fence);
        if block.id == id {
            return info;
        }
        if matches!(info.kind, LineKind::FenceLine) {
            inside_code_fence = !inside_code_fence;
        }
    }

    analyze_line("", false)
}

fn strip_paragraph_wrapper(rendered: &str) -> Option<&str> {
    rendered
        .trim_end_matches('\n')
        .strip_prefix("<p>")
        .and_then(|inner| inner.strip_suffix("</p>"))
}

fn contains_disallowed_block_html(rendered: &str) -> bool {
    let lowered = rendered.to_ascii_lowercase();
    [
        "<p",
        "</p",
        "<ul",
        "<ol",
        "<li",
        "<blockquote",
        "<h1",
        "<h2",
        "<h3",
        "<h4",
        "<h5",
        "<h6",
        "<table",
        "<thead",
        "<tbody",
        "<tr",
        "<td",
        "<th",
        "<pre",
        "<hr",
        "<div",
    ]
    .iter()
    .any(|tag| lowered.contains(tag))
}

fn render_inline_html(text: &str) -> String {
    if text.is_empty() {
        return "<br/>".to_string();
    }

    let mut rendered = String::new();
    html::push_html(&mut rendered, Parser::new_ext(text, markdown_options()));
    let rendered = strip_paragraph_wrapper(&rendered)
        .unwrap_or(rendered.trim_end_matches('\n'))
        .trim();

    if rendered.is_empty() {
        return "<br/>".to_string();
    }

    if contains_disallowed_block_html(rendered) {
        html_escape(text)
    } else {
        rendered.to_string()
    }
}

fn render_line_html(info: &LineRenderInfo) -> String {
    match info.kind {
        LineKind::Blank => "<br/>".to_string(),
        LineKind::Hr => "<span class=\"editor-inline-rule\"></span>".to_string(),
        LineKind::FenceLine | LineKind::CodeFenceBody | LineKind::RawOnly => {
            html_escape(&info.presentation.visible_text)
        }
        _ => render_inline_html(&info.presentation.visible_text),
    }
}

fn visible_cursor_to_raw_cursor(
    prefix_len: usize,
    text: &str,
    visible_utf16_offset: usize,
) -> usize {
    let prefix = utf16_len(&text[..prefix_len.min(text.len())]);
    prefix + visible_utf16_offset
}

fn preserve_hidden_prefix(prefix_len: usize, previous_text: &str, visible_text: &str) -> String {
    if prefix_len > 0 {
        format!(
            "{}{}",
            &previous_text[..prefix_len.min(previous_text.len())],
            visible_text
        )
    } else {
        visible_text.to_string()
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn utf16_offset_to_byte_index(text: &str, utf16_offset: usize) -> usize {
    if utf16_offset == 0 {
        return 0;
    }

    let mut seen = 0;
    for (byte_idx, ch) in text.char_indices() {
        if seen >= utf16_offset {
            return byte_idx;
        }
        let next = seen + ch.len_utf16();
        if utf16_offset < next {
            return byte_idx + ch.len_utf8();
        }
        seen = next;
    }

    text.len()
}

fn get_text_cursor_offset(el: &web_sys::HtmlElement) -> Option<usize> {
    let doc = web_sys::window()?.document()?;
    let sel = doc.get_selection().ok()??;
    if sel.range_count() == 0 {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let pre = doc.create_range().ok()?;
    pre.set_start(el, 0).ok()?;
    pre.set_end(&range.start_container().ok()?, range.start_offset().ok()?)
        .ok()?;
    let text: String = pre.to_string().into();
    Some(utf16_len(&text))
}

fn find_text_position(node: &web_sys::Node, remaining: &mut usize) -> Option<(web_sys::Node, u32)> {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        let len = utf16_len(&node.text_content().unwrap_or_default());
        if *remaining <= len {
            return Some((node.clone(), *remaining as u32));
        }
        *remaining -= len;
        return None;
    }

    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.get(i) {
            if let Some(result) = find_text_position(&child, remaining) {
                return Some(result);
            }
        }
    }
    None
}

fn set_text_cursor_offset(el: &web_sys::HtmlElement, offset: usize) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(Some(sel)) = doc.get_selection() else {
        return;
    };

    let mut remaining = offset;
    if let Some((node, local_off)) = find_text_position(el.as_ref(), &mut remaining) {
        if let Ok(range) = doc.create_range() {
            let _ = range.set_start(&node, local_off);
            let _ = range.collapse_with_to_start(true);
            let _ = sel.remove_all_ranges();
            let _ = sel.add_range(&range);
        }
    } else if let Ok(range) = doc.create_range() {
        let _ = range.select_node_contents(el);
        let _ = range.collapse_with_to_start(false);
        let _ = sel.remove_all_ranges();
        let _ = sel.add_range(&range);
    }
}

fn request_animation_frame(f: impl FnOnce() + 'static) {
    use wasm_bindgen::closure::Closure;

    if let Some(window) = web_sys::window() {
        let callback = Closure::once_into_js(f);
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
    }
}

fn focus_block_with_offset(block_id: usize, offset: usize) {
    request_animation_frame(move || {
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Some(el) = doc.get_element_by_id(&format!("line-{block_id}")) {
                if let Ok(html_el) = el.dyn_into::<web_sys::HtmlElement>() {
                    let _ = html_el.focus();
                    set_text_cursor_offset(&html_el, offset);
                }
            }
        }
    });
}

#[component]
pub fn EditorBlockComponent(
    state: EditorState,
    block: TextBlock,
    #[prop(into)] on_change: Callback<(), ()>,
    #[prop(into)] on_at_menu: Callback<(i32, i32, usize), ()>,
) -> impl IntoView {
    let id = block.id;
    let (focused, set_focused) = signal(false);
    let line_ref = NodeRef::<leptos::html::Div>::new();

    let is_raw = move || {
        let info = state.blocks.with(|blocks| line_render_info(blocks, id));
        info.raw_only
            || state.active_raw_line.get() == Some(id)
            || state.raw_lines.with(|lines| lines.contains(&id))
    };

    Effect::new(move |_| {
        let current_text = state.blocks.with(|blocks| current_block_text(blocks, id));
        let info = state.blocks.with(|blocks| line_render_info(blocks, id));
        let raw = is_raw();

        if let Some(el) = line_ref.get() {
            let html_el: web_sys::HtmlElement = el.clone().into();
            let cursor = focused
                .get()
                .then(|| get_text_cursor_offset(&html_el))
                .flatten();

            if raw {
                let dom_text = html_el
                    .text_content()
                    .unwrap_or_default()
                    .replace('\u{200b}', "");
                if dom_text != current_text {
                    html_el.set_text_content(Some(&current_text));
                }
            } else {
                html_el.set_inner_html(&render_line_html(&info));
            }

            if let Some(offset) = cursor {
                let max_offset = if raw {
                    utf16_len(&current_text)
                } else {
                    utf16_len(&info.presentation.visible_text)
                };
                set_text_cursor_offset(&html_el, offset.min(max_offset));
            }
        }
    });

    let on_focus = move |_ev: ev::FocusEvent| {
        set_focused.set(true);
        let info = state.blocks.with(|blocks| line_render_info(blocks, id));
        let pinned_raw = state.raw_lines.with(|lines| lines.contains(&id));
        if info.raw_only || info.auto_raw_on_focus || pinned_raw || !info.hidden_prefix {
            state.active_raw_line.set(Some(id));
        }
    };

    let on_blur = move |_ev: ev::FocusEvent| {
        set_focused.set(false);
        if state.active_raw_line.get_untracked() == Some(id) {
            state.active_raw_line.set(None);
        }
    };

    let on_input = move |ev: ev::Event| {
        let target = event_target::<web_sys::HtmlElement>(&ev);
        let raw_mode = is_raw();
        let cursor_utf16 = get_text_cursor_offset(&target);
        let visible = target
            .text_content()
            .unwrap_or_default()
            .replace('\u{200b}', "");
        let (previous_text, previous_info) = state
            .blocks
            .with(|blocks| (current_block_text(blocks, id), line_render_info(blocks, id)));

        let next_text = if raw_mode || previous_info.raw_only {
            visible.clone()
        } else {
            preserve_hidden_prefix(
                previous_info.presentation.prefix_len,
                &previous_text,
                &visible,
            )
        };

        state.blocks.update(|blocks| {
            if let Some(block) = blocks.iter_mut().find(|block| block.id == id) {
                block.text = next_text.clone();
            }
        });

        if let Some(cursor_utf16) = cursor_utf16 {
            let cursor_source_utf16 = if raw_mode {
                cursor_utf16
            } else {
                visible_cursor_to_raw_cursor(
                    previous_info.presentation.prefix_len,
                    &previous_text,
                    cursor_utf16,
                )
            };
            let cursor = utf16_offset_to_byte_index(&next_text, cursor_source_utf16);
            if cursor > 0 && cursor <= next_text.len() && next_text[..cursor].ends_with('@') {
                let rect = target.get_bounding_client_rect();
                let blocks = state.blocks.get_untracked();
                let pos = blocks.iter().position(|block| block.id == id).unwrap_or(0);
                let prev_len: usize = blocks[..pos].iter().map(|block| block.text.len() + 1).sum();
                on_at_menu.run((
                    rect.left() as i32 + 20,
                    rect.bottom() as i32 + 8,
                    prev_len + cursor,
                ));
            }
        }

        on_change.run(());
    };

    let on_keydown = move |ev: ev::KeyboardEvent| {
        let Some(target) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
        else {
            return;
        };

        let (current_text, current_info) = state
            .blocks
            .with(|blocks| (current_block_text(blocks, id), line_render_info(blocks, id)));
        let raw_mode = is_raw();

        if (ev.ctrl_key() || ev.meta_key()) && (ev.key() == "r" || ev.key() == "R") {
            if current_info.raw_only {
                return;
            }
            ev.prevent_default();
            state.raw_lines.update(|lines| {
                if !lines.insert(id) {
                    lines.remove(&id);
                }
            });
            state.active_raw_line.set(Some(id));
            return;
        }

        if !raw_mode
            && current_info.hidden_prefix
            && (ev.key() == "ArrowLeft" || ev.key() == "Home")
        {
            let cursor = get_text_cursor_offset(&target).unwrap_or_default();
            if cursor == 0 {
                ev.prevent_default();
                state.active_raw_line.set(Some(id));
                focus_block_with_offset(
                    id,
                    visible_cursor_to_raw_cursor(
                        current_info.presentation.prefix_len,
                        &current_text,
                        0,
                    ),
                );
                return;
            }
        }

        if ev.key() == "ArrowUp" {
            let cursor = get_text_cursor_offset(&target).unwrap_or_default();
            let blocks = state.blocks.get_untracked();
            if let Some(pos) = blocks.iter().position(|block| block.id == id) {
                if cursor == 0 && pos > 0 {
                    ev.prevent_default();
                    let prev = &blocks[pos - 1];
                    focus_block_with_offset(prev.id, utf16_len(&prev.text));
                    return;
                }
            }
        }

        if ev.key() == "ArrowDown" {
            let cursor = get_text_cursor_offset(&target).unwrap_or_default();
            let blocks = state.blocks.get_untracked();
            if let Some(pos) = blocks.iter().position(|block| block.id == id) {
                let line_len = if raw_mode {
                    utf16_len(&blocks[pos].text)
                } else {
                    utf16_len(&current_info.presentation.visible_text)
                };
                if cursor >= line_len && pos + 1 < blocks.len() {
                    ev.prevent_default();
                    focus_block_with_offset(blocks[pos + 1].id, 0);
                    return;
                }
            }
        }

        if ev.key() == "Enter" && !ev.shift_key() {
            ev.prevent_default();
            let visible_cursor = get_text_cursor_offset(&target).unwrap_or_else(|| {
                if raw_mode {
                    utf16_len(&current_text)
                } else {
                    utf16_len(&current_info.presentation.visible_text)
                }
            });
            let split_at_utf16 = if raw_mode {
                visible_cursor
            } else {
                visible_cursor_to_raw_cursor(
                    current_info.presentation.prefix_len,
                    &current_text,
                    visible_cursor,
                )
            };
            let split_at = utf16_offset_to_byte_index(&current_text, split_at_utf16);
            let (before, after) = if split_at <= current_text.len() {
                (
                    current_text[..split_at].to_string(),
                    current_text[split_at..].to_string(),
                )
            } else {
                (current_text.clone(), String::new())
            };

            let new_id = state.next_id.get();
            state.next_id.update(|next| *next += 1);
            state.blocks.update(|blocks| {
                if let Some(pos) = blocks.iter().position(|block| block.id == id) {
                    blocks[pos].text = before.clone();
                    blocks.insert(
                        pos + 1,
                        TextBlock {
                            id: new_id,
                            text: after,
                        },
                    );
                }
            });
            on_change.run(());
            focus_block_with_offset(new_id, 0);
            return;
        }

        if ev.key() == "Backspace" {
            let cursor = get_text_cursor_offset(&target).unwrap_or_default();
            if !raw_mode && current_info.hidden_prefix && cursor == 0 {
                ev.prevent_default();
                state.active_raw_line.set(Some(id));
                focus_block_with_offset(
                    id,
                    visible_cursor_to_raw_cursor(
                        current_info.presentation.prefix_len,
                        &current_text,
                        0,
                    ),
                );
                return;
            }
            if cursor == 0 {
                let mut merge = None;
                state.blocks.update(|blocks| {
                    if let Some(pos) = blocks.iter().position(|block| block.id == id) {
                        if pos > 0 {
                            ev.prevent_default();
                            let previous_id = blocks[pos - 1].id;
                            let previous_len = utf16_len(&blocks[pos - 1].text);
                            let current_text = blocks[pos].text.clone();
                            blocks[pos - 1].text.push_str(&current_text);
                            blocks.remove(pos);
                            merge = Some((previous_id, previous_len));
                        }
                    }
                });

                if let Some((previous_id, previous_len)) = merge {
                    on_change.run(());
                    focus_block_with_offset(previous_id, previous_len);
                }
            }
        }
    };

    view! {
        <div
            class=move || {
                state
                    .blocks
                    .with(|blocks| line_render_info(blocks, id).presentation.row_classes)
            }
            class:raw=is_raw
            class:focused=move || focused.get()
        >
            <div class="editor-line-rail" aria-hidden="true">
                <span class="editor-line-marker">
                    {move || {
                        if is_raw() {
                            String::new()
                        } else {
                            state
                                .blocks
                                .with(|blocks| line_render_info(blocks, id).presentation.marker_text)
                                .unwrap_or_default()
                        }
                    }}
                </span>
            </div>
            <div class="editor-line-frame">
                <div
                    id=format!("line-{id}")
                    node_ref=line_ref
                    class="editor-line"
                    class:is-empty=move || {
                        if is_raw() {
                            state
                                .blocks
                                .with(|blocks| current_block_text(blocks, id).is_empty())
                        } else {
                            state.blocks.with(|blocks| {
                                let info = line_render_info(blocks, id);
                                info.presentation.visible_text.is_empty()
                                    && !matches!(info.kind, LineKind::Hr)
                            })
                        }
                    }
                    class:show-placeholder=move || {
                        state.blocks.with(|blocks| {
                            blocks.len() == 1
                                && blocks
                                    .first()
                                    .map(|block| block.id == id && block.text.is_empty())
                                    .unwrap_or(false)
                        })
                    }
                    contenteditable="true"
                    spellcheck="false"
                    data-placeholder="Type @ to insert"
                    on:focus=on_focus
                    on:blur=on_blur
                    on:input=on_input
                    on:keydown=on_keydown
                ></div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: usize, text: &str) -> TextBlock {
        TextBlock {
            id,
            text: text.to_string(),
        }
    }

    #[test]
    fn classifies_heading_rows() {
        let info = analyze_line("## Heading", false);
        assert_eq!(info.kind, LineKind::Heading(2));
        assert_eq!(info.presentation.prefix_len, 3);
        assert_eq!(info.presentation.visible_text, "Heading");
        assert!(info.hidden_prefix);
        assert!(!info.raw_only);
    }

    #[test]
    fn classifies_bullet_rows() {
        let info = analyze_line("- item", false);
        assert_eq!(info.kind, LineKind::BulletItem);
        assert_eq!(info.presentation.visible_text, "item");
        assert_eq!(info.presentation.marker_text.as_deref(), Some("•"));
        assert!(info.hidden_prefix);
    }

    #[test]
    fn classifies_ordered_rows() {
        let info = analyze_line("12. item", false);
        assert_eq!(info.kind, LineKind::OrderedItem);
        assert_eq!(info.presentation.visible_text, "item");
        assert_eq!(info.presentation.marker_text.as_deref(), Some("12."));
        assert!(info.hidden_prefix);
    }

    #[test]
    fn classifies_task_rows() {
        let info = analyze_line("- [x] done", false);
        assert_eq!(info.kind, LineKind::TaskItem { checked: true });
        assert_eq!(info.presentation.visible_text, "done");
        assert_eq!(info.presentation.marker_text.as_deref(), Some("☑"));
        assert!(info.hidden_prefix);
    }

    #[test]
    fn classifies_quote_rows() {
        let info = analyze_line("> quoted", false);
        assert_eq!(info.kind, LineKind::QuoteLine);
        assert_eq!(info.presentation.visible_text, "quoted");
        assert!(info.hidden_prefix);
        assert!(!info.raw_only);
    }

    #[test]
    fn classifies_horizontal_rules() {
        let info = analyze_line("---", false);
        assert_eq!(info.kind, LineKind::Hr);
        assert!(info.hidden_prefix);
        assert!(info.auto_raw_on_focus);
        assert_eq!(info.presentation.visible_text, "");
    }

    #[test]
    fn classifies_fence_rows_and_bodies() {
        let open = analyze_line("```rust", false);
        assert_eq!(open.kind, LineKind::FenceLine);
        assert!(open.raw_only);

        let body = analyze_line("let x = 1;", true);
        assert_eq!(body.kind, LineKind::CodeFenceBody);
        assert!(body.raw_only);

        let close = analyze_line("```", true);
        assert_eq!(close.kind, LineKind::FenceLine);
        assert!(close.raw_only);
    }

    #[test]
    fn classifies_blank_rows() {
        let info = analyze_line("", false);
        assert_eq!(info.kind, LineKind::Blank);
        assert_eq!(info.presentation.visible_text, "");
        assert!(!info.raw_only);
    }

    #[test]
    fn falls_back_to_raw_for_table_rows() {
        let info = analyze_line("| a | b |", false);
        assert_eq!(info.kind, LineKind::RawOnly);
        assert!(info.raw_only);
    }

    #[test]
    fn falls_back_to_raw_for_html_blocks() {
        let info = analyze_line("<section>", false);
        assert_eq!(info.kind, LineKind::RawOnly);
        assert!(info.raw_only);
    }

    #[test]
    fn code_fence_state_tracks_across_blocks() {
        let blocks = vec![block(0, "```"), block(1, "let x = 1;"), block(2, "```")];
        assert_eq!(line_render_info(&blocks, 1).kind, LineKind::CodeFenceBody);
        assert_eq!(line_render_info(&blocks, 2).kind, LineKind::FenceLine);
    }

    #[test]
    fn hidden_prefix_cursor_mapping_uses_prefix_length() {
        let info = analyze_line("### Heading", false);
        assert_eq!(
            visible_cursor_to_raw_cursor(info.presentation.prefix_len, "### Heading", 0),
            utf16_len("### ")
        );
        assert_eq!(
            visible_cursor_to_raw_cursor(info.presentation.prefix_len, "### Heading", 4),
            utf16_len("### Head")
        );
    }

    #[test]
    fn preserve_hidden_prefix_keeps_markdown_controls() {
        let info = analyze_line("- item", false);
        assert_eq!(
            preserve_hidden_prefix(info.presentation.prefix_len, "- item", "updated"),
            "- updated"
        );
    }

    #[test]
    fn inline_renderer_drops_block_wrappers() {
        let html = render_inline_html("Hello **bold** [link](https://example.com)");
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<a href=\"https://example.com\">link</a>"));
        assert!(!html.contains("<p>"));
        assert!(!html.contains("<ul>"));
        assert!(!html.contains("<blockquote>"));
    }
}
