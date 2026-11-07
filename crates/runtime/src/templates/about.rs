/// What a template says of itself in its `template.json`, which a new project does not get.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct About {
    pub(crate) name: String,
    pub(crate) description: String,
}
