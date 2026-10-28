use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub const CLOVY_OBSIDIAN_SKILL_ID: &str = "clovy-obsidian";
/// The id a profile copied from a June-era install may still store or hold on
/// disk. It is read and mapped to the Clovy id, never written.
pub const LEGACY_OBSIDIAN_SKILL_ID: &str = "june-obsidian";

pub fn canonical_skill_id(skill_id: &str) -> &str {
    if skill_id == LEGACY_OBSIDIAN_SKILL_ID {
        CLOVY_OBSIDIAN_SKILL_ID
    } else {
        skill_id
    }
}

pub fn canonical_skill_ids<'a>(skill_ids: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    skill_ids
        .into_iter()
        .map(canonical_skill_id)
        .map(str::to_string)
        .collect()
}

pub fn read_skill_ids(skill_id: &str) -> Vec<&str> {
    let canonical = canonical_skill_id(skill_id);
    if canonical == CLOVY_OBSIDIAN_SKILL_ID {
        vec![CLOVY_OBSIDIAN_SKILL_ID, LEGACY_OBSIDIAN_SKILL_ID]
    } else {
        vec![canonical]
    }
}

pub fn write_skill_ids(skill_id: &str) -> Vec<&str> {
    vec![canonical_skill_id(skill_id)]
}

pub fn skill_file(root: &Path, skill_id: &str) -> PathBuf {
    root.join(skill_id).join("SKILL.md")
}

/// Reconciles the managed Obsidian skill before the bundled resource is seeded:
/// a June-era copy is read once into the Clovy id and never written again.
pub fn reconcile_managed_obsidian_skill(root: &Path) -> io::Result<()> {
    let canonical = skill_file(root, CLOVY_OBSIDIAN_SKILL_ID);
    let legacy = skill_file(root, LEGACY_OBSIDIAN_SKILL_ID);
    if canonical.is_file() || !legacy.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(&legacy)?;
    write_skill_file(&canonical, &content, CLOVY_OBSIDIAN_SKILL_ID)
}

pub fn skill_content_for_id(content: &str, skill_id: &str) -> String {
    let mut lines = content.split_inclusive('\n').peekable();
    let Some(first) = lines.next() else {
        return format!("---\nname: {skill_id}\n---\n");
    };
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return format!("---\nname: {skill_id}\n---\n\n{content}");
    }

    let newline = if first.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = first.to_string();
    let mut found_name = false;
    let mut closed_frontmatter = false;
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !closed_frontmatter && trimmed.trim() == "---" {
            if !found_name {
                output.push_str(&format!("name: {skill_id}{newline}"));
            }
            output.push_str(line);
            closed_frontmatter = true;
            continue;
        }
        if !closed_frontmatter
            && trimmed
                .split_once(':')
                .is_some_and(|(key, _)| key.trim() == "name")
        {
            let indentation = &trimmed[..trimmed.len() - trimmed.trim_start().len()];
            output.push_str(indentation);
            output.push_str("name: ");
            output.push_str(skill_id);
            if line.ends_with("\r\n") {
                output.push_str("\r\n");
            } else if line.ends_with('\n') {
                output.push('\n');
            }
            found_name = true;
        } else {
            output.push_str(line);
        }
    }
    output
}

fn write_skill_file(destination: &Path, content: &str, skill_id: &str) -> io::Result<()> {
    let content = skill_content_for_id(content, skill_id);
    if fs::read_to_string(destination).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(".SKILL.md.clovy-identity.tmp");
    fs::write(&temporary, content)?;
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_june_era_skill_is_read_once_into_the_clovy_id() {
        let root = tempfile::tempdir().expect("managed skill root");
        let legacy = skill_file(root.path(), LEGACY_OBSIDIAN_SKILL_ID);
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy directory");
        let original = "---\nname: june-obsidian\ndescription: User description\ncustom: keep-me\n---\n\nUser-edited body\n";
        fs::write(&legacy, original).expect("legacy skill");

        reconcile_managed_obsidian_skill(root.path()).expect("read legacy skill");

        let canonical = fs::read_to_string(skill_file(root.path(), CLOVY_OBSIDIAN_SKILL_ID))
            .expect("canonical skill");
        assert!(canonical.contains("name: clovy-obsidian"));
        assert!(canonical.contains("custom: keep-me"));
        assert!(canonical.ends_with("User-edited body\n"));
        assert_eq!(
            fs::read_to_string(&legacy).expect("legacy untouched"),
            original
        );
    }

    #[test]
    fn the_clovy_skill_is_never_mirrored_back() {
        let root = tempfile::tempdir().expect("managed skill root");
        let canonical = skill_file(root.path(), CLOVY_OBSIDIAN_SKILL_ID);
        fs::create_dir_all(canonical.parent().expect("canonical parent"))
            .expect("canonical directory");
        fs::write(&canonical, "---\nname: clovy-obsidian\n---\n\ninstructions")
            .expect("canonical skill");

        reconcile_managed_obsidian_skill(root.path()).expect("reconcile");

        assert!(!skill_file(root.path(), LEGACY_OBSIDIAN_SKILL_ID).exists());
        assert_eq!(
            write_skill_ids(LEGACY_OBSIDIAN_SKILL_ID),
            vec![CLOVY_OBSIDIAN_SKILL_ID]
        );
    }

    #[test]
    fn stored_june_era_ids_read_as_the_clovy_id() {
        assert_eq!(
            canonical_skill_ids([LEGACY_OBSIDIAN_SKILL_ID, "research"]),
            BTreeSet::from([CLOVY_OBSIDIAN_SKILL_ID.to_string(), "research".to_string()])
        );
    }
}
