const MAX_PROMPT_TERMS: usize = 80;
const MAX_PROMPT_CHARS: usize = 1_200;

pub fn build_initial_prompt(terms: &[String]) -> Option<String> {
    let mut cleaned_terms = terms
        .iter()
        .map(|term| term.replace('\0', "").trim().to_string())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    cleaned_terms.sort_by_key(|term| term.to_ascii_lowercase());
    cleaned_terms.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    if cleaned_terms.is_empty() {
        return None;
    }

    let mut prompt = String::from("Use these custom vocabulary terms exactly when heard: ");
    let mut selected = Vec::new();

    for term in cleaned_terms.into_iter().take(MAX_PROMPT_TERMS) {
        let next_len = prompt.len() + selected.join(", ").len() + term.len() + 2;
        if next_len > MAX_PROMPT_CHARS {
            break;
        }
        selected.push(term);
    }

    if selected.is_empty() {
        None
    } else {
        prompt.push_str(&selected.join(", "));
        prompt.push('.');
        Some(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::build_initial_prompt;

    #[test]
    fn prompt_uses_terms_and_removes_duplicates() {
        let prompt = build_initial_prompt(&[
            "Tauri".to_string(),
            " tauri ".to_string(),
            "Whisper".to_string(),
        ])
        .expect("prompt");

        assert!(prompt.contains("Tauri"));
        assert!(prompt.contains("Whisper"));
        assert_eq!(prompt.matches("Tauri").count(), 1);
    }

    #[test]
    fn empty_terms_do_not_create_prompt() {
        assert_eq!(None, build_initial_prompt(&[" ".to_string()]));
    }
}
