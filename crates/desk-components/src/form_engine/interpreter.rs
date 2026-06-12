use dioxus::prelude::*;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::form_engine::live_form::{FieldDef, FieldType};
use std::collections::HashMap;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ClientFieldSchema {
    pub fieldname: String,
    pub label: String,
    pub fieldtype: String, // "Data", "Int", "Currency", "Link"
    pub reqd: bool,
}

#[derive(Props, Clone, PartialEq)]
pub struct FormProps {
    pub doctype_name: String,
    pub document_id: String,
    #[props(optional)]
    pub fields: Option<Vec<FieldDef>>,
    #[props(optional)]
    pub on_save: Option<EventHandler<serde_json::Map<String, Value>>>,
    #[props(optional)]
    pub link_options: Option<HashMap<String, Vec<String>>>,
}

#[component]
pub fn DynamicFormInterpreter(props: FormProps) -> Element {
    let mut document_state = use_signal(serde_json::Map::new);
    let mut is_saving = use_signal(|| false);
    let mut save_status = use_signal(|| "".to_string());

    // Compute active fields list
    let fields = props.fields.clone().unwrap_or_else(|| {
        vec![
            FieldDef {
                fieldname: "customer".to_string(),
                label: "Customer".to_string(),
                fieldtype: FieldType::Link,
                required: true,
                read_only: false,
                hidden: false,
                options: Some("Customer".to_string()),
                default: None,
                description: Some("Select the primary customer account".to_string()),
            },
            FieldDef {
                fieldname: "posting_date".to_string(),
                label: "Posting Date".to_string(),
                fieldtype: FieldType::Date,
                required: true,
                read_only: false,
                hidden: false,
                options: None,
                default: None,
                description: None,
            },
            FieldDef {
                fieldname: "amount".to_string(),
                label: "Amount".to_string(),
                fieldtype: FieldType::Currency,
                required: true,
                read_only: false,
                hidden: false,
                options: None,
                default: None,
                description: Some("Specify transaction amount in USD".to_string()),
            },
            FieldDef {
                fieldname: "status".to_string(),
                label: "Status".to_string(),
                fieldtype: FieldType::Select,
                required: false,
                read_only: false,
                hidden: false,
                options: Some("Draft\nSubmitted\nCancelled".to_string()),
                default: Some("Draft".to_string()),
                description: None,
            },
            FieldDef {
                fieldname: "notes".to_string(),
                label: "Notes".to_string(),
                fieldtype: FieldType::Data,
                required: false,
                read_only: false,
                hidden: false,
                options: None,
                default: None,
                description: Some("Additional notes or logs".to_string()),
            },
        ]
    });

    // Populate default values on mount
    let mut first_mount = use_signal(|| true);
    if *first_mount.read() {
        let mut state = document_state.write();
        for f in &fields {
            if let Some(ref def) = f.default {
                state.insert(f.fieldname.clone(), Value::String(def.clone()));
            }
        }
        first_mount.set(false);
    }

    let handle_save = move |_| {
        is_saving.set(true);
        save_status.set("Saving Document...".to_string());
        
        let data = document_state.read().clone();
        
        if let Some(ref handler) = props.on_save {
            handler.call(data);
        }
        
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            is_saving.set(false);
            save_status.set("Document Saved Successfully!".to_string());
        });
    };

    rsx! {
        div {
            class: "max-w-4xl mx-auto bg-slate-900 rounded-3xl border border-slate-800 shadow-xl overflow-hidden font-sans text-white",
            
            // Header
            div { class: "p-6 border-b border-slate-800/80 bg-slate-950 flex justify-between items-center",
                div {
                    h2 { class: "text-xl font-bold tracking-tight text-white", "{props.doctype_name}" }
                    p { class: "text-xs text-slate-500 mt-1", "ID: {props.document_id}" }
                }
                span { class: "px-3 py-1 rounded-full text-xs font-semibold bg-blue-500/10 text-blue-400 border border-blue-500/20", "Interpreter Mode" }
            }

            // Grid of fields
            div { class: "p-6 grid grid-cols-1 md:grid-cols-2 gap-6",
                for field in fields.iter().filter(|f| !f.hidden) {
                    div { class: "space-y-1.5",
                        label { class: "block text-xs font-bold text-slate-400 uppercase tracking-wider",
                            "{field.label}"
                            if field.required {
                                span { class: "text-red-500 ml-1", "*" }
                            }
                        }
                        
                        FieldWidget {
                            field: field.clone(),
                            value: document_state.read().get(&field.fieldname).cloned().unwrap_or(Value::Null),
                            link_options: props.link_options.clone(),
                            on_change: {
                                let fieldname = field.fieldname.clone();
                                move |new_val: Value| {
                                    document_state.write().insert(fieldname.clone(), new_val);
                                }
                            }
                        }
                        
                        if let Some(ref desc) = field.description {
                            p { class: "text-[11px] text-slate-500", "{desc}" }
                        }
                    }
                }
            }

            // Actions panel
            div { class: "p-6 border-t border-slate-800/80 bg-slate-950 flex flex-col sm:flex-row items-center justify-between gap-4",
                span { class: "text-xs font-medium text-slate-400", "{save_status}" }
                
                button {
                    class: format!(
                        "px-6 py-3 text-sm font-bold rounded-xl transition duration-150 flex items-center justify-center space-x-2 text-white shadow-lg {}",
                        if *is_saving.read() { "bg-blue-600/50 cursor-not-allowed" } else { "bg-blue-600 hover:bg-blue-500 hover:shadow-blue-500/15" }
                    ),
                    disabled: *is_saving.read(),
                    onclick: handle_save,
                    if *is_saving.read() { "Saving..." } else { "Save Draft" }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct DynamicFormProps {
    pub fields: Vec<ClientFieldSchema>,
    #[props(optional)]
    pub on_change: Option<EventHandler<serde_json::Map<String, Value>>>,
}

#[component]
pub fn DynamicForm(props: DynamicFormProps) -> Element {
    let mut document_state = use_signal(serde_json::Map::new);
    let mut is_saving = use_signal(|| false);
    let mut save_status = use_signal(|| "".to_string());

    let fields: Vec<FieldDef> = props.fields.iter().map(|f| {
        let ft = match f.fieldtype.as_str() {
            "Data" => FieldType::Data,
            "Int" => FieldType::Int,
            "Currency" => FieldType::Currency,
            "Link" => FieldType::Link,
            _ => FieldType::Data,
        };
        FieldDef {
            fieldname: f.fieldname.clone(),
            label: f.label.clone(),
            fieldtype: ft,
            required: f.reqd,
            read_only: false,
            hidden: false,
            options: if f.fieldtype == "Link" { Some("Customer".to_string()) } else { None },
            default: None,
            description: None,
        }
    }).collect();

    let handle_save = move |_| {
        is_saving.set(true);
        save_status.set("Saving Document...".to_string());
        
        let data = document_state.read().clone();
        
        if let Some(ref handler) = props.on_change {
            handler.call(data);
        }
        
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            is_saving.set(false);
            save_status.set("Document Saved Successfully!".to_string());
        });
    };

    rsx! {
        div {
            class: "max-w-4xl mx-auto bg-slate-900 rounded-3xl border border-slate-800 shadow-xl overflow-hidden font-sans text-white",
            
            // Header
            div { class: "p-6 border-b border-slate-800/80 bg-slate-950 flex justify-between items-center",
                div {
                    h2 { class: "text-xl font-bold tracking-tight text-white", "Dynamic Form Engine" }
                    p { class: "text-xs text-slate-500 mt-1", "Metadata-driven client form compilation" }
                }
            }

            // Grid of fields
            div { class: "p-6 grid grid-cols-1 md:grid-cols-2 gap-6",
                for field in fields.iter() {
                    div { class: "space-y-1.5",
                        label { class: "block text-xs font-bold text-slate-400 uppercase tracking-wider",
                            "{field.label}"
                            if field.required {
                                span { class: "text-red-500 ml-1", "*" }
                            }
                        }
                        
                        FieldWidget {
                            field: field.clone(),
                            value: document_state.read().get(&field.fieldname).cloned().unwrap_or(Value::Null),
                            link_options: None,
                            on_change: {
                                let fieldname = field.fieldname.clone();
                                let on_change_cb = props.on_change;
                                move |new_val: Value| {
                                    document_state.write().insert(fieldname.clone(), new_val);
                                    if let Some(ref cb) = on_change_cb {
                                        cb.call(document_state.read().clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Actions panel
            div { class: "p-6 border-t border-slate-800/80 bg-slate-950 flex flex-col sm:flex-row items-center justify-between gap-4",
                span { class: "text-xs font-medium text-slate-400", "{save_status}" }
                
                button {
                    class: format!(
                        "px-6 py-3 text-sm font-bold rounded-xl transition duration-150 flex items-center justify-center space-x-2 text-white shadow-lg {}",
                        if *is_saving.read() { "bg-blue-600/50 cursor-not-allowed" } else { "bg-blue-600 hover:bg-blue-500 hover:shadow-blue-500/15" }
                    ),
                    disabled: *is_saving.read(),
                    onclick: handle_save,
                    if *is_saving.read() { "Saving..." } else { "Save Draft" }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct FieldWidgetProps {
    field: FieldDef,
    value: Value,
    link_options: Option<HashMap<String, Vec<String>>>,
    on_change: EventHandler<Value>,
}

#[component]
fn FieldWidget(props: FieldWidgetProps) -> Element {
    let field = &props.field;
    let str_val = match &props.value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => "".to_string(),
    };
    let bool_val = matches!(&props.value, Value::Bool(true));

    let input_style = "w-full bg-slate-950 border border-slate-800 rounded-xl p-3 text-sm text-white placeholder-slate-500 outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 transition";

    match field.fieldtype {
        FieldType::Check => rsx! {
            label { class: "flex items-center space-x-2.5 cursor-pointer py-1.5",
                input {
                    r#type: "checkbox",
                    checked: bool_val,
                    disabled: field.read_only,
                    class: "w-4 h-4 rounded border-slate-800 bg-slate-950 text-blue-600 focus:ring-blue-500/20 focus:ring-offset-0 focus:ring-2 accent-blue-600 cursor-pointer",
                    onchange: move |e| props.on_change.call(Value::Bool(e.checked()))
                }
                span { class: "text-sm text-slate-300 font-medium", "{field.label}" }
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
                    class: input_style,
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
                class: format!("{} min-h-[100px] resize-y", input_style),
                disabled: field.read_only,
                value: str_val.clone(),
                oninput: move |e| props.on_change.call(Value::String(e.value()))
            }
        },

        FieldType::Date => rsx! {
            input {
                class: input_style,
                r#type: "date",
                value: str_val.clone(),
                disabled: field.read_only,
                oninput: move |e| props.on_change.call(Value::String(e.value()))
            }
        },

        FieldType::Datetime => rsx! {
            input {
                class: input_style,
                r#type: "datetime-local",
                value: str_val.clone(),
                disabled: field.read_only,
                oninput: move |e| props.on_change.call(Value::String(e.value()))
            }
        },

        FieldType::Int => rsx! {
            input {
                class: input_style,
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

        FieldType::Float => rsx! {
            input {
                class: input_style,
                r#type: "number",
                step: "0.01",
                value: str_val.clone(),
                disabled: field.read_only,
                oninput: move |e| {
                    let v = e.value().parse::<f64>().ok()
                        .and_then(serde_json::Number::from_f64)
                        .map(Value::Number)
                        .unwrap_or(Value::Null);
                    props.on_change.call(v);
                }
            }
        },

        FieldType::Currency => rsx! {
            CurrencyFieldWidget {
                field: field.clone(),
                value: props.value.clone(),
                on_change: props.on_change,
            }
        },

        FieldType::Link => {
            let target_doctype = field.options.clone().unwrap_or_default();
            let options = if let Some(ref map) = props.link_options {
                map.get(&target_doctype).cloned().unwrap_or_else(|| get_default_link_options(&target_doctype))
            } else {
                get_default_link_options(&target_doctype)
            };
            
            rsx! {
                LinkFieldWidget {
                    field: field.clone(),
                    value: props.value.clone(),
                    options,
                    on_change: props.on_change,
                }
            }
        },

        _ => rsx! {
            input {
                class: input_style,
                r#type: "text",
                value: str_val.clone(),
                disabled: field.read_only,
                oninput: move |e| props.on_change.call(Value::String(e.value()))
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct CurrencyProps {
    field: FieldDef,
    value: Value,
    on_change: EventHandler<Value>,
}

#[component]
fn CurrencyFieldWidget(props: CurrencyProps) -> Element {
    let mut error_msg = use_signal(|| None::<String>);
    let str_val = match &props.value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };

    let mut handle_input = move |val_str: String| {
        if val_str.is_empty() {
            error_msg.set(None);
            props.on_change.call(Value::Null);
            return;
        }
        match val_str.parse::<f64>() {
            Ok(val) => {
                if val < 0.0 {
                    error_msg.set(Some("Currency amount must be positive".to_string()));
                    props.on_change.call(Value::String(val_str));
                } else {
                    error_msg.set(None);
                    if let Some(num) = serde_json::Number::from_f64(val) {
                        props.on_change.call(Value::Number(num));
                    } else {
                        props.on_change.call(Value::String(val_str));
                    }
                }
            }
            Err(_) => {
                error_msg.set(Some("Must be a valid numeric amount".to_string()));
                props.on_change.call(Value::String(val_str));
            }
        }
    };

    rsx! {
        div { class: "space-y-1 w-full",
            div { class: "relative",
                span { class: "absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-500 text-sm font-semibold select-none", "$" }
                input {
                    class: format!(
                        "w-full bg-slate-950 border rounded-xl pl-8 pr-3 py-3 text-sm text-white placeholder-slate-500 outline-none transition {}",
                        if error_msg.read().is_some() { "border-red-500 ring-2 ring-red-500/20" } else { "border-slate-800 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20" }
                    ),
                    r#type: "number",
                    step: "0.01",
                    placeholder: "0.00",
                    value: "{str_val}",
                    disabled: props.field.read_only,
                    oninput: move |e| handle_input(e.value())
                }
            }
            if let Some(ref err) = *error_msg.read() {
                p { class: "text-[11px] text-red-500 font-medium", "{err}" }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct LinkProps {
    field: FieldDef,
    value: Value,
    options: Vec<String>,
    on_change: EventHandler<Value>,
}

#[component]
fn LinkFieldWidget(props: LinkProps) -> Element {
    let mut search_query = use_signal(|| "".to_string());
    let mut dropdown_open = use_signal(|| false);

    let str_val = match &props.value {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };

    let query = search_query.read().to_lowercase();
    let filtered: Vec<String> = props.options
        .iter()
        .filter(|opt| opt.to_lowercase().contains(&query))
        .cloned()
        .collect();

    let display_val = if *dropdown_open.read() {
        search_query.read().clone()
    } else {
        str_val.clone()
    };

    let open_dropdown = move |_| {
        search_query.set(str_val.clone());
        dropdown_open.set(true);
    };

    rsx! {
        div { class: "relative w-full",
            if *dropdown_open.read() {
                div {
                    class: "fixed inset-0 z-30",
                    onclick: move |_| dropdown_open.set(false),
                }
            }
            
            div { class: "relative z-40",
                input {
                    class: format!(
                        "w-full bg-slate-950 border rounded-xl p-3 text-sm text-white placeholder-slate-500 outline-none transition {}",
                        if *dropdown_open.read() { "border-blue-500 ring-2 ring-blue-500/20" } else { "border-slate-800 hover:border-slate-700" }
                    ),
                    r#type: "text",
                    placeholder: format!("Search {}...", props.field.options.as_deref().unwrap_or("")),
                    value: "{display_val}",
                    onfocus: open_dropdown,
                    oninput: move |evt| {
                        search_query.set(evt.value());
                        dropdown_open.set(true);
                    }
                }
                
                if *dropdown_open.read() {
                    div {
                        class: "absolute left-0 right-0 mt-1 bg-slate-900 border border-slate-800 rounded-xl shadow-2xl max-h-60 overflow-y-auto z-50 p-1.5 space-y-1",
                        if filtered.is_empty() {
                            div { class: "p-3 text-xs text-slate-500 text-center", "No matches found" }
                        } else {
                            for opt in filtered {
                                button {
                                    r#type: "button",
                                    class: "w-full text-left p-2.5 text-sm rounded-lg hover:bg-slate-800 transition text-slate-300 hover:text-white font-medium",
                                    onclick: {
                                        let opt = opt.clone();
                                        move |_| {
                                            props.on_change.call(Value::String(opt.clone()));
                                            search_query.set(opt.clone());
                                            dropdown_open.set(false);
                                        }
                                    },
                                    "{opt}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn get_default_link_options(target: &str) -> Vec<String> {
    match target {
        "Customer" => vec![
            "Cust-001 (Acme Corp)".to_string(),
            "Cust-002 (Globex)".to_string(),
            "Cust-003 (Stark Industries)".to_string(),
            "Cust-004 (Initech)".to_string(),
            "Cust-005 (Umbrella Corp)".to_string(),
        ],
        "Item" => vec![
            "Item-001 (Processor)".to_string(),
            "Item-002 (RAM 16GB)".to_string(),
            "Item-003 (SSD 1TB)".to_string(),
            "Item-004 (Motherboard)".to_string(),
        ],
        _ => vec![
            format!("{}-001", target),
            format!("{}-002", target),
            format!("{}-003", target),
        ],
    }
}
