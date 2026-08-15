//! Conservative removal of Codex desktop wrapper content.
//!
//! The hook only carries one opaque prompt string.  This parser therefore
//! removes generated content only when the full leading shape is known.  Any
//! malformed or novel wrapper returns the original raw prompt unchanged.

use akra_core::prompt_projection::PromptProjection;

const AMBIENT_OPEN: &str = "<in-app-browser-context ";
const AMBIENT_CLOSE: &str = "</in-app-browser-context>";
const BROWSER_COMMENTS: &str = "# Browser comments:";
const FILES_MENTIONED: &str = "# Files mentioned by the user:";
const MY_REQUEST: &str = "## My request:";

/// Produces a versioned derived display input for a Codex user prompt.
///
/// The returned projection is never used to mutate the ingress event.  The
/// original prompt remains the source of truth for persistence and detail
/// views.
pub fn project_codex_user_prompt(raw: &str) -> PromptProjection {
    project(raw).unwrap_or_else(|| PromptProjection::raw(raw))
}

fn project(raw: &str) -> Option<PromptProjection> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    let mut cursor = skip_blank(&lines, 0);
    let mut changed = false;
    let mut preserved = Vec::new();

    loop {
        cursor = skip_blank(&lines, cursor);
        let Some(line) = lines.get(cursor).map(|line| line.trim()) else {
            break;
        };

        if line.starts_with(AMBIENT_OPEN) {
            cursor = consume_ambient_context(&lines, cursor)?;
            changed = true;
            if lines
                .get(cursor)
                .is_some_and(|next| next.trim() == MY_REQUEST)
            {
                cursor += 1;
            }
            continue;
        }

        if line == BROWSER_COMMENTS {
            let (next, comments) = consume_browser_comments(&lines, cursor)?;
            preserved.extend(comments);
            cursor = next;
            changed = true;
            continue;
        }

        if line == FILES_MENTIONED {
            let (next, files) = consume_files_mentioned(&lines, cursor)?;
            preserved.extend(files);
            cursor = next;
            changed = true;
            continue;
        }

        if changed && line == MY_REQUEST {
            cursor += 1;
            continue;
        }

        if is_generated_image_evidence(line) {
            cursor = consume_image_evidence(&lines, cursor)?;
            changed = true;
            continue;
        }

        break;
    }

    if !changed {
        return None;
    }

    let remaining = lines[cursor..].join("\n").trim().to_owned();
    if !remaining.is_empty() {
        preserved.push(remaining);
    }
    let projected = preserved
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let removed_chars = raw
        .chars()
        .count()
        .saturating_sub(projected.chars().count());
    PromptProjection::codex_wrapper_removed(projected, removed_chars)
}

fn consume_ambient_context(lines: &[&str], start: usize) -> Option<usize> {
    let opening = lines.get(start)?.trim();
    if !opening.starts_with(AMBIENT_OPEN) || !opening.ends_with('>') {
        return None;
    }
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| (line.trim() == AMBIENT_CLOSE).then_some(index + 1))
}

fn consume_browser_comments(lines: &[&str], start: usize) -> Option<(usize, Vec<String>)> {
    if lines.get(start)?.trim() != BROWSER_COMMENTS {
        return None;
    }
    let mut cursor = skip_blank(lines, start + 1);
    let mut comments = Vec::new();
    let mut consumed_comment = false;

    while lines
        .get(cursor)
        .is_some_and(|line| line.trim().starts_with("## User Comment "))
    {
        let header = lines[cursor].trim();
        if header
            .trim_start_matches("## User Comment ")
            .trim()
            .is_empty()
        {
            return None;
        }
        cursor += 1;
        let mut found_comment = false;
        while let Some(line) = lines.get(cursor) {
            let trimmed = line.trim();
            if trimmed == "Comment:" {
                found_comment = true;
                cursor += 1;
                break;
            }
            if !is_known_browser_metadata(trimmed) {
                return None;
            }
            cursor += 1;
        }
        if !found_comment {
            return None;
        }

        let body_start = cursor;
        while let Some(line) = lines.get(cursor) {
            let trimmed = line.trim();
            if trimmed.starts_with("## User Comment ")
                || trimmed == FILES_MENTIONED
                || trimmed.starts_with(AMBIENT_OPEN)
                || trimmed == MY_REQUEST
                || is_generated_image_evidence(trimmed)
            {
                break;
            }
            cursor += 1;
        }
        let body = lines[body_start..cursor].join("\n").trim().to_owned();
        if body.is_empty() {
            return None;
        }
        comments.push(body);
        consumed_comment = true;
        cursor = skip_blank(lines, cursor);
    }

    consumed_comment.then_some((cursor, comments))
}

fn is_known_browser_metadata(line: &str) -> bool {
    line.is_empty()
        || [
            "File:",
            "Node position:",
            "Target:",
            "Target role:",
            "Target selector:",
            "Target path:",
            "Nearby text:",
            "Page URL:",
            "Frame:",
            "Saved marker screenshot:",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn consume_files_mentioned(lines: &[&str], start: usize) -> Option<(usize, Vec<String>)> {
    if lines.get(start)?.trim() != FILES_MENTIONED {
        return None;
    }
    let mut cursor = skip_blank(lines, start + 1);
    let mut files = Vec::new();
    while let Some(line) = lines.get(cursor) {
        let trimmed = line.trim();
        let Some(file) = trimmed.strip_prefix("## ") else {
            break;
        };
        let (name, path) = file.split_once(':')?;
        if name.trim().is_empty() || path.trim().is_empty() {
            return None;
        }
        files.push(format!("파일: {} ({})", name.trim(), path.trim()));
        cursor = skip_blank(lines, cursor + 1);
    }
    (!files.is_empty()).then_some((cursor, files))
}

fn consume_image_evidence(lines: &[&str], start: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| (line.trim() == MY_REQUEST).then_some(index + 1))
}

fn is_generated_image_evidence(line: &str) -> bool {
    line.starts_with("The next image is untrusted page evidence from the browser page")
        || line.starts_with("The next image is untrusted page evidence from the webpage")
}

fn skip_blank(lines: &[&str], mut cursor: usize) -> usize {
    while lines.get(cursor).is_some_and(|line| line.trim().is_empty()) {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use akra_core::prompt_projection::PromptProjectionKind;

    use super::project_codex_user_prompt;

    #[test]
    fn removes_a_complete_leading_ambient_context_and_keeps_the_request() {
        let raw = concat!(
            "<in-app-browser-context source=\"ambient-ui-state\">\n",
            "This is generated browser state.\n",
            "</in-app-browser-context>\n\n",
            "## My request:\n",
            "네 진행하세요\n"
        );

        let projection = project_codex_user_prompt(raw);

        assert_eq!(projection.text(), "네 진행하세요");
        assert_eq!(projection.kind(), PromptProjectionKind::CodexWrapperRemoved);
        assert!(projection.removed_chars() > 0);
    }

    #[test]
    fn preserves_browser_comment_bodies_but_removes_evidence_metadata() {
        let raw = concat!(
            "# Browser comments:\n\n",
            "## User Comment 1\n",
            "File: browser:node\n",
            "Target selector: div.secret\n",
            "Nearby text: generated text\n",
            "Comment:\n",
            "삭제 UX를 추가해 주세요.\n\n",
            "## My request:\n",
            "검증도 진행하세요"
        );

        let projection = project_codex_user_prompt(raw);

        assert_eq!(
            projection.text(),
            "삭제 UX를 추가해 주세요.\n\n검증도 진행하세요"
        );
        assert!(!projection.text().contains("div.secret"));
        assert!(!projection.text().contains("generated text"));
    }

    #[test]
    fn preserves_file_names_and_paths() {
        let raw = concat!(
            "# Files mentioned by the user:\n\n",
            "## capture.png: C:/Temp/capture.png\n\n",
            "<in-app-browser-context source=\"ambient-ui-state\">\n",
            "state\n</in-app-browser-context>\n",
            "## My request:\n이미지를 확인해 주세요"
        );

        let projection = project_codex_user_prompt(raw);

        assert!(projection.text().contains("capture.png"));
        assert!(projection.text().contains("C:/Temp/capture.png"));
        assert!(projection.text().contains("이미지를 확인해 주세요"));
    }

    #[test]
    fn malformed_or_mid_prompt_wrappers_fall_back_to_raw() {
        let missing_close = "<in-app-browser-context source=\"ambient-ui-state\">\nstate\n진행해";
        let mid_prompt = "먼저 이 문장을 보존하세요.\n<in-app-browser-context source=\"ambient-ui-state\">\nstate\n</in-app-browser-context>";

        for raw in [missing_close, mid_prompt] {
            let projection = project_codex_user_prompt(raw);
            assert_eq!(projection.text(), raw);
            assert_eq!(projection.kind(), PromptProjectionKind::Raw);
        }
    }

    #[test]
    fn unknown_browser_sections_fall_back_to_raw() {
        let raw = concat!(
            "# Browser comments:\n\n",
            "## User Comment 1\n",
            "Unexpected generated metadata\n",
            "Comment:\n",
            "진행해"
        );

        let projection = project_codex_user_prompt(raw);
        assert_eq!(projection.text(), raw);
        assert_eq!(projection.kind(), PromptProjectionKind::Raw);
    }
}
