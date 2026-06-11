use dioxus::prelude::*;
use desk_components::form_engine::interpreter::DynamicFormInterpreter;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        div {
            DynamicFormInterpreter {
                doctype_name: "Sales Invoice".to_string(),
                document_id: "INV-2026-0001".to_string(),
            }
        }
    }
}
