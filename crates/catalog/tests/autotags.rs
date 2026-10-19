use lightcraft_catalog::{
    Catalog, Filter, Meta, Op, Photo, PhotoId, Rule, Sort, Source,
    autotags::{AutoTag, AutoTags},
    keywords::KeywordInfo,
};
use serde_json::json;

fn catalog() -> Catalog {
    let mut c = Catalog::new();
    let mut p = Photo::new(PhotoId(1), Source::File { path: "/offline/photo.jpg".into() }, "photo.jpg", "JPEG", 100, 100, "2026-10-19");
    p.content_hash = Some("pixels-v1".into());
    p.meta = Meta {
        keywords: vec!["Portfolio".into()],
        auto_tags: Some(AutoTags {
            model: "tinyclip-8m@revision".into(),
            content: p.content_key(),
            tags: vec![AutoTag { name: "Places|Church".into(), score: 0.32, model: "tinyclip-8m@revision".into(), accepted: false }],
            dismissed: vec![],
        }),
        ..Default::default()
    };
    p.rating = 4;
    c.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
    c.apply(Op::SetKeyword { path: "Places|Church".into(), info: Some(KeywordInfo { synonyms: vec!["Kapelle".into()], ..Default::default() }) })
        .unwrap();
    c
}

#[test]
fn generated_keywords_use_existing_search_filters_counts_and_synonyms() {
    let c = catalog();
    let p = c.photo(PhotoId(1)).unwrap();
    for text in
        ["CHURCH", "chu", "kapelle", "keyword:Places", "kw:places|church", "tag:places|church", "church rating:>3 portfolio", "\"places|church\""]
    {
        assert_eq!(c.query(&Filter { text: text.into(), ..Default::default() }, &Sort::default()), vec![p.id], "{text}");
    }
    assert!(Filter { keyword: Some("Places".into()), ..Default::default() }.matches(p, &c));
    assert_eq!(p.keyword_count(), 1);
    for (field, op, value) in [
        ("keywords", "contains", json!("church")),
        ("keywords", "is", json!("Kapelle")),
        ("keywordCount", "is", json!(1)),
        ("text", "contains", json!("kapelle")),
    ] {
        assert!(Rule::Field { field: field.into(), op: op.into(), value }.matches(p, &c));
    }
    let tree = c.keyword_tree();
    let places = tree.iter().find(|n| n.name == "Places").unwrap();
    assert_eq!(places.count, 0);
    assert_eq!(places.children[0].count, 0);
    assert_eq!(c.export_keywords(&p.meta.keywords).flat, vec!["Portfolio"]);
}

#[test]
fn reopening_old_catalogs_and_source_invalidation() {
    let c = catalog();
    let reopened = Catalog::from_snapshot(&c.to_snapshot()).unwrap();
    assert_eq!(reopened.photo(PhotoId(1)).unwrap().meta, c.photo(PhotoId(1)).unwrap().meta);
    let mut snapshot: serde_json::Value = serde_json::from_str(&c.to_snapshot()).unwrap();
    fn remove_ai(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("auto_tags");
                for v in m.values_mut() {
                    remove_ai(v);
                }
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    remove_ai(v);
                }
            }
            _ => {}
        }
    }
    remove_ai(&mut snapshot);
    let old = Catalog::from_snapshot(&snapshot.to_string()).unwrap();
    assert!(old.photo(PhotoId(1)).unwrap().meta.auto_tags.is_none());
    let mut p = c.photo(PhotoId(1)).unwrap().as_ref().clone();
    p.content_hash = Some("pixels-v2".into());
    assert_eq!(p.suggested_tags().count(), 0);
    assert_eq!(p.keyword_count(), 1);
}

#[test]
fn ordinary_keyword_edits_leave_predictions_independent_and_keep_accepted_provenance() {
    let mut c = catalog();
    let id = PhotoId(1);
    let before = c.photo(id).unwrap().meta.auto_tags.clone();
    for op in [c.rename_keyword_ops("Places", "Travel").unwrap(), c.delete_keyword_ops("Places").unwrap()] {
        let undo = c.apply(op).unwrap();
        assert_eq!(c.photo(id).unwrap().meta.auto_tags, before);
        assert_eq!(c.photo(id).unwrap().meta.keywords, ["Portfolio"]);
        assert_eq!(c.query(&Filter { text: "tag:places|church".into(), ..Default::default() }, &Sort::default()), [id]);
        c.apply(undo).unwrap();
    }
    let mut meta = c.photo(id).unwrap().meta.clone();
    meta.keywords.push("Places|Church".into());
    meta.auto_tags.as_mut().unwrap().tags[0].accepted = true;
    c.apply(Op::SetMeta { id, meta: Box::new(meta) }).unwrap();
    let undo = c.apply(c.rename_keyword_ops("Places", "Travel").unwrap()).unwrap();
    let photo = c.photo(id).unwrap();
    assert!(photo.meta.keywords.contains(&"Travel|Church".into()));
    let tag = &photo.meta.auto_tags.as_ref().unwrap().tags[0];
    assert!(tag.accepted && tag.name == "Travel|Church" && tag.model == "tinyclip-8m@revision");
    c.apply(undo).unwrap();
    let undo = c.apply(c.delete_keyword_ops("Places").unwrap()).unwrap();
    assert_eq!(c.photo(id).unwrap().meta.keywords, ["Portfolio"]);
    c.apply(undo).unwrap();
    assert!(c.photo(id).unwrap().meta.keywords.contains(&"Places|Church".into()));
}

#[test]
fn ordinary_keyword_semantics_stay_the_same() {
    let mut c = catalog();
    let mut meta = c.photo(PhotoId(1)).unwrap().meta.clone();
    meta.auto_tags = None;
    c.apply(Op::SetMeta { id: PhotoId(1), meta: Box::new(meta) }).unwrap();
    assert!(c.query(&Filter { text: "church".into(), ..Default::default() }, &Sort::default()).is_empty());
    assert_eq!(c.query(&Filter { text: "PORTFOLIO rating:4".into(), ..Default::default() }, &Sort::default()), vec![PhotoId(1)]);
}

#[test]
fn combined_search_requires_every_subject_and_scene_and_survives_reopening() {
    let mut c = Catalog::new();
    for (id, names) in [(1, &["cat", "chair", "indoors"][..]), (2, &["cat"][..]), (3, &["chair"][..]), (4, &["cat", "chair", "garden"][..])] {
        let mut p = Photo::new(PhotoId(id), Source::File { path: format!("/offline/{id}.jpg") }, "photo.jpg", "JPEG", 512, 512, "2026-10-19");
        p.meta.auto_tags = Some(AutoTags {
            model: "index-v2".into(),
            content: p.content_key(),
            tags: names.iter().map(|name| AutoTag { name: (*name).into(), score: 0.3, model: "index-v2".into(), accepted: false }).collect(),
            dismissed: vec![],
        });
        c.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
    }
    let reopened = Catalog::from_snapshot(&c.to_snapshot()).unwrap();
    for c in [&c, &reopened] {
        for text in ["cat chair", "CAT CHAIR", "tag:cat tag:chair", "chair cat"] {
            let mut found = c.query(&Filter { text: text.into(), ..Default::default() }, &Sort::default());
            found.sort();
            assert_eq!(found, [PhotoId(1), PhotoId(4)], "{text}");
        }
        assert_eq!(c.query(&Filter { text: "cat chair indoors".into(), ..Default::default() }, &Sort::default()), [PhotoId(1)]);
        assert_eq!(c.query(&Filter { text: "cat chair garden".into(), ..Default::default() }, &Sort::default()), [PhotoId(4)]);
        assert!(c.query(&Filter { text: "cat chair dog".into(), ..Default::default() }, &Sort::default()).is_empty());
    }
}

#[test]
fn ai_metadata_stays_searchable_without_creating_ordinary_keyword_entries() {
    let mut c = catalog();
    c.apply(Op::SetKeyword { path: "Places|Church".into(), info: None }).unwrap();
    assert!(c.keyword_tree().iter().all(|n| n.name != "Places"));
    assert!(c.keyword_path("Places|Church").is_none());
    assert_eq!(c.keywords(), [("Portfolio".into(), 1)]);
    assert!(c.keyword_suggestions(&[], "church", 10).is_empty());
    assert_eq!(c.query(&Filter { text: "church tag:places|church".into(), ..Default::default() }, &Sort::default()), [PhotoId(1)]);
    let mut meta = c.photo(PhotoId(1)).unwrap().meta.clone();
    meta.keywords.push("Places|Church".into());
    meta.auto_tags.as_mut().unwrap().tags[0].accepted = true;
    c.apply(Op::SetMeta { id: PhotoId(1), meta: Box::new(meta) }).unwrap();
    assert_eq!(c.keyword_tree().iter().find(|n| n.name == "Places").unwrap().count, 1);
    assert_eq!(c.photo(PhotoId(1)).unwrap().keyword_count(), 2);
    assert_eq!(c.keyword_path("places|church").as_deref(), Some("Places|Church"));
}
