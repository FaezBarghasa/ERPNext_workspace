use dioxus::prelude::*;
use serde_json::Value;

#[derive(Props, Clone, PartialEq)]
pub struct FormProps {
    pub doctype_name: String,
    pub document_id: String,
}

#[component]
pub fn DynamicFormInterpreter(props: FormProps) -> Element {
    let mut document_state = use_signal(serde_json::Map::new);
    let mut is_saving = use_signal(|| false);

    let mut on_field_change = move |field_name: String, value: String| {
        document_state.write().insert(field_name, Value::String(value));
    };

    rsx! {
        div { class: "p-6 max-w-4xl mx-auto bg-white rounded-xl shadow-md space-y-4",
            h2 { class: "text-2xl font-bold text-gray-900 border-b pb-2",
                "{props.doctype_name}: {props.document_id}"
            }
            div { class: "grid grid-cols-2 gap-4",
                div { class: "flex flex-col space-y-1",
                    label { class: "text-sm font-medium text-gray-700", "Customer" }
                    input { 
                        class: "border rounded p-2 focus:ring focus:ring-blue-200 outline-none",
                        r#type: "text",
                        placeholder: "Link to Customer...",
                        oninput: move |evt| on_field_change("customer".to_string(), evt.value())
                    }
                }
                div { class: "flex flex-col space-y-1",
                    label { class: "text-sm font-medium text-gray-700", "Posting Date" }
                    input { 
                        class: "border rounded p-2 focus:ring focus:ring-blue-200 outline-none",
                        r#type: "date",
                        oninput: move |evt| on_field_change("posting_date".to_string(), evt.value())
                    }
                }
            }

            button {
                class: "mt-4 bg-blue-600 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded shadow transition duration-150",
                onclick: move |_| {
                    is_saving.set(true);
                },
                if *is_saving.read() { "Saving Draft..." } else { "Save Document" }
            }
        }
    }
}
