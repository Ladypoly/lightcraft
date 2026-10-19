//! Portable AI annotations in lc:aiTags; ordinary dc:subject stays user-controlled.
use lightcraft_catalog::{
    Photo,
    autotags::{AutoTag, AutoTags},
};
use lightcraft_meta::XmpValue;

const MAX_TAGS: usize = 512;

fn text(value: &XmpValue, name: &str, limit: usize) -> Option<String> {
    let text = value.field(name)?.text()?;
    (text.len() <= limit && !text.chars().any(char::is_control)).then(|| text.to_string())
}

pub(super) fn read(value: &XmpValue) -> Option<AutoTags> {
    if value.field("lc:version")?.text()? != "1" {
        return None;
    }
    let content = text(value, "lc:sourceHash", 128)?;
    if content.len() != 32 || !content.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = AutoTags { model: text(value, "lc:index", 2048)?, content, ..Default::default() };
    let tags = value.field("lc:tags")?.items();
    if tags.len() > MAX_TAGS {
        return None;
    }
    for tag in tags {
        let name = text(tag, "lc:name", 512)?;
        let score: f32 = tag.field("lc:score")?.text()?.parse().ok()?;
        if name.trim().is_empty() || !score.is_finite() || !(-1.0..=1.0).contains(&score) {
            return None;
        }
        out.tags.push(AutoTag {
            name,
            score,
            model: text(tag, "lc:model", 512)?,
            accepted: match tag.field("lc:accepted")?.text()? {
                "True" | "true" => true,
                "False" | "false" => false,
                _ => return None,
            },
        });
    }
    if let Some(dismissed) = value.field("lc:dismissed") {
        if dismissed.items().len() > MAX_TAGS {
            return None;
        }
        for name in dismissed.items() {
            let name = name.text()?;
            if name.len() > 512 || name.chars().any(char::is_control) {
                return None;
            }
            out.dismissed.push(name.into());
        }
    }
    out.tags.retain(|t| !out.dismissed.iter().any(|n| lightcraft_catalog::keywords::same(n, &t.name)));
    Some(out)
}

fn structure(fields: impl IntoIterator<Item = (&'static str, XmpValue)>) -> XmpValue {
    XmpValue::Struct(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

pub(super) fn write(photo: &Photo) -> Option<XmpValue> {
    let ai = photo.meta.auto_tags.as_ref()?;
    // File identity travels across renames/copies, and prevents old sidecars tagging replacement pixels.
    if photo.content_hash.as_deref() != Some(ai.content.as_str()) {
        return None;
    }
    let text = |s: &str| XmpValue::Text(s.into());
    let value = structure([
        ("lc:version", text("1")),
        ("lc:sourceHash", text(&ai.content)),
        ("lc:index", text(&ai.model)),
        (
            "lc:tags",
            XmpValue::Array(
                ai.tags
                    .iter()
                    .map(|tag| {
                        structure([
                            ("lc:name", text(&tag.name)),
                            ("lc:score", text(&tag.score.to_string())),
                            ("lc:model", text(&tag.model)),
                            ("lc:accepted", text(if tag.accepted { "True" } else { "False" })),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("lc:dismissed", XmpValue::Array(ai.dismissed.iter().map(|s| text(s)).collect())),
    ]);
    // Apply the same bounds on output so every packet we write can be read back.
    read(&value).map(|_| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_or_excessive_annotations() {
        let mut photo = Photo::new(
            lightcraft_catalog::PhotoId(1),
            lightcraft_catalog::Source::File { path: "/photo.png".into() },
            "photo.png",
            "PNG",
            1,
            1,
            "now",
        );
        photo.content_hash = Some("0123456789abcdef0123456789abcdef".into());
        photo.meta.auto_tags = Some(AutoTags {
            model: "model-v1".into(),
            content: photo.content_key(),
            tags: vec![AutoTag { name: "cat <chair> & café".into(), score: 0.35, model: "tinyclip-8m".into(), accepted: false }],
            dismissed: vec!["dog".into()],
        });
        let valid = write(&photo).unwrap();
        assert_eq!(read(&valid), photo.meta.auto_tags);
        let packet = lightcraft_meta::write_xmp_lc_values(&Default::default(), &[("aiTags", valid.clone())]);
        let parsed = lightcraft_meta::parse_xmp(&packet).unwrap();
        assert_eq!(read(&parsed.values["lc:aiTags"]), photo.meta.auto_tags);
        assert!(parsed.metadata.keywords.is_empty());
        for (field, value) in [
            ("lc:version", XmpValue::Text("2".into())),
            ("lc:sourceHash", XmpValue::Text("file:/private/path".into())),
            ("lc:index", XmpValue::Text("x".repeat(2049))),
            ("lc:tags", XmpValue::Array(vec![valid.clone(); MAX_TAGS + 1])),
        ] {
            let XmpValue::Struct(mut fields) = valid.clone() else { panic!() };
            fields.insert(field.into(), value);
            assert!(read(&XmpValue::Struct(fields)).is_none(), "{field}");
        }
        for score in [f32::NAN, f32::INFINITY, -2.0, 1.1] {
            photo.meta.auto_tags.as_mut().unwrap().tags[0].score = score;
            assert!(write(&photo).is_none());
        }
        photo.meta.auto_tags.as_mut().unwrap().tags.clear();
        photo.meta.auto_tags.as_mut().unwrap().dismissed.clear();
        let packet = lightcraft_meta::write_xmp_lc_values(&Default::default(), &[("aiTags", write(&photo).unwrap())]);
        let parsed = lightcraft_meta::parse_xmp(&packet.replace("lc:", "alt:").replace("xmlns:lc=", "xmlns:alt=")).unwrap();
        assert_eq!(read(&parsed.values["lc:aiTags"]), photo.meta.auto_tags, "empty results and alternative prefixes round-trip");
    }
}
