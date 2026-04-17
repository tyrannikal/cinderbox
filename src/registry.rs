use crate::Language;

pub struct LanguageSpec {
    pub categories: &'static [ToolCategory],
    pub common_deps: &'static [CommonDep],
}

pub struct ToolCategory {
    pub name: &'static str,
    pub tools: &'static [Tool],
}

pub struct Tool {
    pub id: &'static str,
    pub label: &'static str,
    pub default_pre_commit: bool,
    pub default_ci: bool,
}

pub struct CommonDep {
    pub id: &'static str,
    pub label: &'static str,
}

pub fn spec_for(language: Language) -> &'static LanguageSpec {
    match language {
        Language::Python => &PYTHON_SPEC,
        Language::Rust => &RUST_SPEC,
        _ => &EMPTY_SPEC,
    }
}

/// Look up a tool by id across all language specs. Returns None if the tool isn't
/// in any registry.
pub fn tool_by_id(id: &str) -> Option<&'static Tool> {
    for lang in [Language::Python, Language::Rust] {
        for cat in spec_for(lang).categories {
            for tool in cat.tools {
                if tool.id == id {
                    return Some(tool);
                }
            }
        }
    }
    None
}

pub const EMPTY_SPEC: LanguageSpec = LanguageSpec {
    categories: &[],
    common_deps: &[],
};

const PYTHON_LINTERS: &[Tool] = &[
    Tool { id: "ruff",   label: "Ruff",   default_pre_commit: true,  default_ci: true  },
    Tool { id: "black",  label: "Black",  default_pre_commit: true,  default_ci: false },
    Tool { id: "pylint", label: "Pylint", default_pre_commit: false, default_ci: true  },
];
const PYTHON_TYPE_CHECKERS: &[Tool] = &[
    Tool { id: "pyright", label: "Pyright", default_pre_commit: true, default_ci: true },
    Tool { id: "mypy",    label: "Mypy",    default_pre_commit: true, default_ci: true },
];
const PYTHON_SECURITY: &[Tool] = &[
    Tool { id: "bandit", label: "Bandit", default_pre_commit: true, default_ci: true },
];
const PYTHON_DEAD_CODE: &[Tool] = &[
    Tool { id: "vulture", label: "Vulture", default_pre_commit: false, default_ci: true },
];
const PYTHON_COMPLEXITY: &[Tool] = &[
    Tool { id: "radon", label: "Radon", default_pre_commit: false, default_ci: true },
    Tool { id: "xenon", label: "Xenon", default_pre_commit: true,  default_ci: true },
];
const PYTHON_TESTS: &[Tool] = &[
    Tool { id: "pytest", label: "pytest", default_pre_commit: false, default_ci: true },
];
const PYTHON_DOCS: &[Tool] = &[
    Tool { id: "mkdocs", label: "MkDocs", default_pre_commit: false, default_ci: false },
];

const PYTHON_CATEGORIES: &[ToolCategory] = &[
    ToolCategory { name: "Linters",       tools: PYTHON_LINTERS },
    ToolCategory { name: "Type checkers", tools: PYTHON_TYPE_CHECKERS },
    ToolCategory { name: "Security",      tools: PYTHON_SECURITY },
    ToolCategory { name: "Dead code",     tools: PYTHON_DEAD_CODE },
    ToolCategory { name: "Complexity",    tools: PYTHON_COMPLEXITY },
    ToolCategory { name: "Tests",         tools: PYTHON_TESTS },
    ToolCategory { name: "Docs",          tools: PYTHON_DOCS },
];

const PYTHON_COMMON_DEPS: &[CommonDep] = &[
    CommonDep { id: "fastapi",    label: "FastAPI" },
    CommonDep { id: "flask",      label: "Flask" },
    CommonDep { id: "django",     label: "Django" },
    CommonDep { id: "pydantic",   label: "Pydantic" },
    CommonDep { id: "requests",   label: "requests" },
    CommonDep { id: "httpx",      label: "httpx" },
    CommonDep { id: "sqlalchemy", label: "SQLAlchemy" },
    CommonDep { id: "numpy",      label: "NumPy" },
    CommonDep { id: "pandas",     label: "pandas" },
];

pub const PYTHON_SPEC: LanguageSpec = LanguageSpec {
    categories: PYTHON_CATEGORIES,
    common_deps: PYTHON_COMMON_DEPS,
};

const RUST_LINTERS: &[Tool] = &[
    Tool { id: "clippy", label: "clippy", default_pre_commit: true, default_ci: true },
];
const RUST_FORMAT: &[Tool] = &[
    Tool { id: "rustfmt", label: "rustfmt", default_pre_commit: true, default_ci: true },
];
const RUST_SECURITY: &[Tool] = &[
    Tool { id: "cargo-audit", label: "cargo-audit", default_pre_commit: false, default_ci: true },
    Tool { id: "cargo-deny",  label: "cargo-deny",  default_pre_commit: false, default_ci: true },
];
const RUST_TESTS: &[Tool] = &[
    Tool { id: "cargo-nextest", label: "cargo-nextest", default_pre_commit: false, default_ci: true },
];
const RUST_COVERAGE: &[Tool] = &[
    Tool { id: "cargo-tarpaulin", label: "cargo-tarpaulin", default_pre_commit: false, default_ci: true },
];
const RUST_DOCS: &[Tool] = &[
    Tool { id: "mdbook", label: "mdBook", default_pre_commit: false, default_ci: false },
];

const RUST_CATEGORIES: &[ToolCategory] = &[
    ToolCategory { name: "Linters",  tools: RUST_LINTERS },
    ToolCategory { name: "Format",   tools: RUST_FORMAT },
    ToolCategory { name: "Security", tools: RUST_SECURITY },
    ToolCategory { name: "Tests",    tools: RUST_TESTS },
    ToolCategory { name: "Coverage", tools: RUST_COVERAGE },
    ToolCategory { name: "Docs",     tools: RUST_DOCS },
];

const RUST_COMMON_DEPS: &[CommonDep] = &[
    CommonDep { id: "tokio",      label: "tokio" },
    CommonDep { id: "serde",      label: "serde" },
    CommonDep { id: "serde_json", label: "serde_json" },
    CommonDep { id: "anyhow",     label: "anyhow" },
    CommonDep { id: "thiserror",  label: "thiserror" },
    CommonDep { id: "clap",       label: "clap" },
    CommonDep { id: "tracing",    label: "tracing" },
];

pub const RUST_SPEC: LanguageSpec = LanguageSpec {
    categories: RUST_CATEGORIES,
    common_deps: RUST_COMMON_DEPS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn spec_for_python_returns_populated_spec() {
        let s = spec_for(Language::Python);
        assert!(!s.categories.is_empty());
        assert!(!s.common_deps.is_empty());
    }

    #[test]
    fn spec_for_rust_returns_populated_spec() {
        let s = spec_for(Language::Rust);
        assert!(!s.categories.is_empty());
        assert!(!s.common_deps.is_empty());
    }

    #[test]
    fn spec_for_go_returns_empty_spec() {
        let s = spec_for(Language::Go);
        assert!(s.categories.is_empty());
        assert!(s.common_deps.is_empty());
    }

    #[test]
    fn spec_for_every_other_language_is_empty() {
        use strum::VariantArray;
        for lang in Language::VARIANTS {
            if matches!(lang, Language::Python | Language::Rust) {
                continue;
            }
            let s = spec_for(*lang);
            assert!(
                s.categories.is_empty() && s.common_deps.is_empty(),
                "expected {lang} to have an empty spec",
            );
        }
    }

    #[test]
    fn all_python_tool_ids_are_unique() {
        let mut seen = HashSet::new();
        for cat in PYTHON_CATEGORIES {
            for tool in cat.tools {
                assert!(seen.insert(tool.id), "duplicate tool id: {}", tool.id);
            }
        }
    }

    #[test]
    fn all_rust_tool_ids_are_unique() {
        let mut seen = HashSet::new();
        for cat in RUST_CATEGORIES {
            for tool in cat.tools {
                assert!(seen.insert(tool.id), "duplicate tool id: {}", tool.id);
            }
        }
    }

    #[test]
    fn tool_by_id_finds_known_tool() {
        let t = tool_by_id("ruff").expect("ruff must be in the registry");
        assert_eq!(t.label, "Ruff");
        assert!(t.default_pre_commit);
    }

    #[test]
    fn tool_by_id_returns_none_for_unknown() {
        assert!(tool_by_id("nonexistent-tool-xyz").is_none());
    }

    #[test]
    fn python_has_all_expected_categories() {
        let s = spec_for(Language::Python);
        let names: Vec<_> = s.categories.iter().map(|c| c.name).collect();
        assert!(names.contains(&"Linters"));
        assert!(names.contains(&"Type checkers"));
        assert!(names.contains(&"Security"));
        assert!(names.contains(&"Tests"));
    }
}
