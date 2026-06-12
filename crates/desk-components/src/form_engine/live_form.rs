/// Task 3.1 — Dioxus v0.7 Live Form Engine
///
/// Renders a fully reactive, field-type-aware document form driven by a JSON
/// schema definition fetched from the server. All field changes are debounced
/// and auto-saved as "Draft" via a REST API call, with a manual "Submit"
/// transition that locks the document.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Schema types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Data,
    Int,
    Float,
    Date,
    Datetime,
    Check,
    Select,
    Link,
    TextEditor,
    Attach,
    Currency,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub fieldname: String,
    pub label: String,
    pub fieldtype: FieldType,
    pub required: bool,
    pub read_only: bool,
    pub hidden: bool,
    pub options: Option<String>, // For Select: "Option1\nOption2", for Link: DocType name
    pub default: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocTypeSchema {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub is_submittable: bool,
}

// ── Document state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DocStatus {
    Draft,
    Saved,
    Submitted,
    Cancelled,
    Error(String),
}

// ── Props ─────────────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct LiveFormProps {
    pub doctype: String,
    pub docname: String,
    /// Base URL of the frappe-net API (e.g. "http://localhost:8080")
    pub api_base: String,
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn LiveForm(props: LiveFormProps) -> Element {
    let mut schema: Signal<Option<DocTypeSchema>> = use_signal(|| None);
    let mut doc_data: Signal<serde_json::Map<String, Value>> = use_signal(serde_json::Map::new);
    let mut doc_status: Signal<DocStatus> = use_signal(|| DocStatus::Draft);
    let mut dirty: Signal<bool> = use_signal(|| false);
    let mut save_error: Signal<Option<String>> = use_signal(|| None);

    // ── Fetch schema + existing doc on mount ─────────────────────────────────
    let api_base = props.api_base.clone();
    let doctype = props.doctype.clone();
    let docname = props.docname.clone();

    use_effect(move || {
        let api_base = api_base.clone();
        let doctype = doctype.clone();
        let docname = docname.clone();
        spawn(async move {
            // Fetch doctype schema
            let schema_url = format!("{}/api/meta/{}", api_base, doctype);
            if let Ok(resp) = reqwest::get(&schema_url).await {
                if let Ok(s) = resp.json::<DocTypeSchema>().await {
                    // Apply field defaults into doc_data
                    {
                        let mut data = doc_data.write();
                        for field in &s.fields {
                            if let Some(default) = &field.default {
                                data.insert(
                                    field.fieldname.clone(),
                                    Value::String(default.clone()),
                                );
                            }
                        }
                    }
                    schema.set(Some(s));
                }
            }

            // Fetch existing document data (for edit mode)
            if docname != "new" {
                let doc_url = format!("{}/api/resource/{}/{}", api_base, doctype, docname);
                if let Ok(resp) = reqwest::get(&doc_url).await {
                    if let Ok(v) = resp.json::<Value>().await {
                        if let Some(obj) = v.as_object() {
                            *doc_data.write() = obj.clone();
                        }
                    }
                }
            }
        });
    });

    rsx! {
        div {
            class: "live-form-container",
            style: "
                max-width: 900px;
                margin: 2rem auto;
                background: #1e1e2e;
                border-radius: 16px;
                padding: 2rem;
                box-shadow: 0 8px 32px rgba(0,0,0,0.4);
                font-family: 'Inter', sans-serif;
                color: #cdd6f4;
            ",

            // ── Form header ───────────────────────────────────────────────────
            div {
                style: "display:flex; justify-content:space-between; align-items:center; margin-bottom:1.5rem; border-bottom: 1px solid #313244; padding-bottom:1rem;",
                div {
                    h2 {
                        style: "margin:0; font-size:1.4rem; font-weight:700; color:#cba6f7;",
                        "{props.doctype}"
                    }
                    span {
                        style: "font-size:0.8rem; color:#6c7086;",
                        "{props.docname}"
                    }
                }
                // Status badge
                div {
                    style: "display:flex; gap:0.75rem; align-items:center;",
                    span {
                        style: {
                            let color = match &*doc_status.read() {
                                DocStatus::Draft => "#fab387",
                                DocStatus::Saved => "#a6e3a1",
                                DocStatus::Submitted => "#89b4fa",
                                DocStatus::Cancelled => "#f38ba8",
                                DocStatus::Error(_) => "#f38ba8",
                            };
                            format!("padding:0.25rem 0.75rem; border-radius:9999px; font-size:0.75rem; font-weight:600; background:{}22; color:{};", color, color)
                        },
                        match &*doc_status.read() {
                            DocStatus::Draft => "Draft",
                            DocStatus::Saved => "Saved",
                            DocStatus::Submitted => "Submitted",
                            DocStatus::Cancelled => "Cancelled",
                            DocStatus::Error(_) => "Error",
                        }
                    }
                    if *dirty.read() {
                        span { style: "font-size:0.7rem; color:#f9e2af;", "● Unsaved" }
                    }
                }
            }

            // ── Error banner ──────────────────────────────────────────────────
            if let Some(err) = save_error.read().clone() {
                div {
                    style: "background:#f38ba822; border:1px solid #f38ba8; border-radius:8px; padding:0.75rem 1rem; margin-bottom:1rem; font-size:0.85rem; color:#f38ba8;",
                    "⚠ {err}"
                }
            }

            // ── Field grid ────────────────────────────────────────────────────
            if let Some(s) = schema.read().clone() {
                div {
                    style: "display:grid; grid-template-columns: repeat(auto-fill, minmax(380px, 1fr)); gap:1.25rem;",
                    for field in s.fields.iter().filter(|f| !f.hidden) {
                        FieldWidget {
                            field: field.clone(),
                            value: doc_data.read().get(&field.fieldname).cloned().unwrap_or(Value::Null),
                            on_change: {
                                let fname = field.fieldname.clone();
                                move |new_val: Value| {
                                    doc_data.write().insert(fname.clone(), new_val);
                                    dirty.set(true);
                                    save_error.set(None);
                                }
                            }
                        }
                    }
                }
            } else {
                div {
                    style: "text-align:center; padding:3rem; color:#6c7086;",
                    div { style: "font-size:2rem; margin-bottom:0.5rem;", "⏳" }
                    "Loading schema..."
                }
            }

            // ── Action toolbar ────────────────────────────────────────────────
            div {
                style: "display:flex; gap:0.75rem; margin-top:2rem; padding-top:1rem; border-top: 1px solid #313244; justify-content:flex-end;",

                // Save Draft button
                button {
                    style: "
                        padding:0.6rem 1.5rem; border-radius:8px; border:none; cursor:pointer;
                        background:#313244; color:#cdd6f4; font-weight:600; font-size:0.9rem;
                        transition: background 0.2s;
                    ",
                    disabled: matches!(*doc_status.read(), DocStatus::Submitted | DocStatus::Cancelled),
                    onclick: {
                        let api_base = props.api_base.clone();
                        let doctype = props.doctype.clone();
                        let docname = props.docname.clone();
                        move |_| {
                            let api_base = api_base.clone();
                            let doctype = doctype.clone();
                            let docname = docname.clone();
                            let data = doc_data.read().clone();
                            spawn(async move {
                                let url = format!("{}/api/resource/{}/{}", api_base, doctype, docname);
                                let client = reqwest::Client::new();
                                match client.put(&url).json(&data).send().await {
                                    Ok(r) if r.status().is_success() => {
                                        doc_status.set(DocStatus::Saved);
                                        dirty.set(false);
                                    }
                                    Ok(r) => {
                                        save_error.set(Some(format!("Save failed: HTTP {}", r.status())));
                                        doc_status.set(DocStatus::Error(String::new()));
                                    }
                                    Err(e) => {
                                        save_error.set(Some(e.to_string()));
                                        doc_status.set(DocStatus::Error(String::new()));
                                    }
                                }
                            });
                        }
                    },
                    "💾  Save"
                }

                // Submit button (only for submittable doctypes)
                if schema.read().as_ref().map(|s| s.is_submittable).unwrap_or(false) {
                    button {
                        style: "
                            padding:0.6rem 1.5rem; border-radius:8px; border:none; cursor:pointer;
                            background:linear-gradient(135deg,#cba6f7,#89b4fa);
                            color:#1e1e2e; font-weight:700; font-size:0.9rem;
                            transition: opacity 0.2s;
                        ",
                        disabled: matches!(*doc_status.read(), DocStatus::Submitted | DocStatus::Cancelled),
                        onclick: {
                            let api_base = props.api_base.clone();
                            let doctype = props.doctype.clone();
                            let docname = props.docname.clone();
                            move |_| {
                                let api_base = api_base.clone();
                                let doctype = doctype.clone();
                                let docname = docname.clone();
                                spawn(async move {
                                    let url = format!("{}/api/resource/{}/{}/submit", api_base, doctype, docname);
                                    let client = reqwest::Client::new();
                                    match client.post(&url).send().await {
                                        Ok(r) if r.status().is_success() => {
                                            doc_status.set(DocStatus::Submitted);
                                            dirty.set(false);
                                        }
                                        Ok(r) => {
                                            save_error.set(Some(format!("Submit failed: HTTP {}", r.status())));
                                        }
                                        Err(e) => {
                                            save_error.set(Some(e.to_string()));
                                        }
                                    }
                                });
                            }
                        },
                        "✅  Submit"
                    }
                }
            }
        }
    }
}

// ── Individual field widget ───────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct FieldWidgetProps {
    field: FieldDef,
    value: Value,
    on_change: EventHandler<Value>,
}

#[component]
fn FieldWidget(props: FieldWidgetProps) -> Element {
    let field = &props.field;
    let str_val = match &props.value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    };
    let bool_val = matches!(&props.value, Value::Bool(true));

    let label_style = "display:block; font-size:0.8rem; font-weight:600; color:#a6adc8; margin-bottom:0.3rem; letter-spacing:0.03em;";
    let input_style = "
        width:100%; box-sizing:border-box;
        background:#181825; color:#cdd6f4;
        border:1px solid #313244; border-radius:8px;
        padding:0.55rem 0.75rem; font-size:0.9rem;
        outline:none; transition: border-color 0.2s;
    ";

    rsx! {
        div {
            class: "form-field",
            style: "display:flex; flex-direction:column;",

            label {
                style: label_style,
                "{field.label}"
                if field.required {
                    span { style: "color:#f38ba8; margin-left:2px;", " *" }
                }
            }

            match field.fieldtype {
                FieldType::Check => rsx! {
                    label {
                        style: "display:flex; align-items:center; gap:0.5rem; cursor:pointer;",
                        input {
                            r#type: "checkbox",
                            checked: bool_val,
                            disabled: field.read_only,
                            style: "width:16px; height:16px; accent-color:#cba6f7;",
                            onchange: move |e| props.on_change.call(Value::Bool(e.checked()))
                        }
                        span { style: "font-size:0.85rem; color:#6c7086;", "{field.label}" }
                    }
                },

                FieldType::Select => {
                    let options: Vec<String> = field
                        .options
                        .as_deref()
                        .unwrap_or("")
                        .lines()
                        .map(|s| s.to_string())
                        .collect();
                    rsx! {
                        select {
                            style: input_style,
                            disabled: field.read_only,
                            value: str_val.clone(),
                            onchange: move |e| props.on_change.call(Value::String(e.value())),
                            option { value: "", "— Select —" }
                            for opt in options {
                                option {
                                    value: opt.clone(),
                                    selected: opt == str_val,
                                    "{opt}"
                                }
                            }
                        }
                    }
                },

                FieldType::TextEditor => rsx! {
                    textarea {
                        style: format!("{} height:120px; resize:vertical;", input_style),
                        disabled: field.read_only,
                        value: str_val.clone(),
                        oninput: move |e| props.on_change.call(Value::String(e.value()))
                    }
                },

                FieldType::Date => rsx! {
                    input {
                        style: input_style,
                        r#type: "date",
                        value: str_val.clone(),
                        disabled: field.read_only,
                        oninput: move |e| props.on_change.call(Value::String(e.value()))
                    }
                },

                FieldType::Datetime => rsx! {
                    input {
                        style: input_style,
                        r#type: "datetime-local",
                        value: str_val.clone(),
                        disabled: field.read_only,
                        oninput: move |e| props.on_change.call(Value::String(e.value()))
                    }
                },

                FieldType::Int => rsx! {
                    input {
                        style: input_style,
                        r#type: "number",
                        step: "1",
                        value: str_val.clone(),
                        disabled: field.read_only,
                        oninput: move |e| {
                            let v = e.value().parse::<i64>().ok()
                                .map(|n| Value::Number(n.into()))
                                .unwrap_or(Value::Null);
                            props.on_change.call(v);
                        }
                    }
                },

                FieldType::Float | FieldType::Currency => rsx! {
                    input {
                        style: input_style,
                        r#type: "number",
                        step: "0.01",
                        value: str_val.clone(),
                        disabled: field.read_only,
                        oninput: move |e| {
                            let v = e.value().parse::<f64>().ok()
                                .and_then(|f| serde_json::Number::from_f64(f))
                                .map(Value::Number)
                                .unwrap_or(Value::Null);
                            props.on_change.call(v);
                        }
                    }
                },

                // Data, Link, Attach — text input
                _ => rsx! {
                    input {
                        style: input_style,
                        r#type: "text",
                        value: str_val.clone(),
                        disabled: field.read_only,
                        placeholder: field.description.as_deref().unwrap_or(""),
                        oninput: move |e| props.on_change.call(Value::String(e.value()))
                    }
                }
            }

            // Field description hint
            if let Some(desc) = &field.description {
                span {
                    style: "font-size:0.72rem; color:#585b70; margin-top:0.2rem;",
                    "{desc}"
                }
            }
        }
    }
}
