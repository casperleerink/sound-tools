//! The sidebar's icons, as lines of 24 points. The agent names a step's icon in words.

use rune::Icon;

macro_rules! icons {
    ($($name:ident = $id:literal $body:literal;)*) => {
        $(pub(super) static $name: Icon = Icon {
            name: $id,
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        };)*
    };
}

icons! {
    PLUS = "plus" r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#;
    CHEVRON_DOWN = "chevron-down" r#"<path d="m6 9 6 6 6-6"/>"#;
    CHEVRON_RIGHT = "chevron-right" r#"<path d="m9 18 6-6-6-6"/>"#;
    ARROW_UP = "arrow-up" r#"<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>"#;
    ARROW_LEFT = "arrow-left" r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#;
    SQUARE = "square" r#"<rect width="18" height="18" x="3" y="3" rx="2"/>"#;
    PAPERCLIP = "paperclip" r#"<path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>"#;
    X = "x" r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#;
    CHECK = "check" r#"<path d="M20 6 9 17l-5-5"/>"#;
    EYE = "eye" r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>"#;
    PENCIL = "pencil" r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>"#;
    TERMINAL = "terminal" r#"<path d="M12 19h8"/><path d="m4 17 6-6-6-6"/>"#;
    SEARCH = "search" r#"<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>"#;
    WRENCH = "wrench" r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>"#;
    REDO = "redo-2" r#"<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"/>"#;
    SHIELD = "shield" r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>"#;
    LOCK = "lock" r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>"#;
    LOCK_OPEN = "lock-open" r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/>"#;
}

/// Claude's mark, in its own orange.
pub(super) static CLAUDE: Icon = Icon {
    name: "claude",
    view: 24.0,
    body: r##"<path fill="#D97757" d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"/>"##,
};

/// Pi's mark, in its own colours.
pub(super) static PI: Icon = Icon {
    name: "pi",
    view: 800.0,
    body: r##"<path fill="#F09082" d="M165.29 165.29H517.36V400H400V282.65H165.29Z"/><path fill="#4D9ABF" d="M165.29 282.65H282.65V400H400V517.36H282.65V634.72H165.29Z"/><path fill="#F1BE58" d="M517.36 400H634.72V634.72H517.36Z"/>"##,
};

/// The icon the agent names for a step or a mode.
pub(super) fn named(name: &str) -> &'static Icon {
    match name {
        "eye" => &EYE,
        "pencil" => &PENCIL,
        "terminal" => &TERMINAL,
        "search" => &SEARCH,
        "redo-2" => &REDO,
        "shield" => &SHIELD,
        "lock" => &LOCK,
        "lock-open" => &LOCK_OPEN,
        _ => &WRENCH,
    }
}

pub(super) fn logo(provider: sound_agent::Provider) -> &'static Icon {
    match provider {
        sound_agent::Provider::Claude => &CLAUDE,
        sound_agent::Provider::Pi => &PI,
    }
}
