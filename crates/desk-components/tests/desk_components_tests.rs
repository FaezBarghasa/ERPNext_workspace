use desk_components::{DynamicFormInterpreter, FormProps, DynamicForm, DynamicFormProps, ClientFieldSchema};
use dioxus::prelude::*;

#[test]
fn test_interpreter_compilation_and_props() {
    let props = FormProps {
        doctype_name: "Customer".to_string(),
        document_id: "CUST-999".to_string(),
        fields: None,
        on_save: None,
        link_options: None,
    };

    assert_eq!(props.doctype_name, "Customer");
    assert_eq!(props.document_id, "CUST-999");
    assert!(props.fields.is_none());

    let mut vdom = VirtualDom::new_with_props(DynamicFormInterpreter, props);
    let _ = vdom.rebuild_in_place();
}

#[test]
fn test_dynamic_form_rendering_and_validation() {
    // 1. Register mock ClientFieldSchema structures
    let fields = vec![
        ClientFieldSchema {
            fieldname: "full_name".to_string(),
            label: "Full Name".to_string(),
            fieldtype: "Data".to_string(),
            reqd: true,
        },
        ClientFieldSchema {
            fieldname: "billing_currency".to_string(),
            label: "Billing Currency".to_string(),
            fieldtype: "Currency".to_string(),
            reqd: true,
        },
        ClientFieldSchema {
            fieldname: "customer_group".to_string(),
            label: "Customer Group".to_string(),
            fieldtype: "Link".to_string(),
            reqd: false,
        },
    ];

    let props = DynamicFormProps {
        fields,
        on_change: None,
    };

    // 2. Perform VirtualDom render
    let mut vdom = VirtualDom::new_with_props(DynamicForm, props);
    let _ = vdom.rebuild_in_place();

    // 3. Render HTML using SSR to assert boundaries
    let html = dioxus_ssr::render(&vdom);
    
    // Assert all fields and labels are rendered correctly
    assert!(html.contains("Full Name"));
    assert!(html.contains("Billing Currency"));
    assert!(html.contains("Customer Group"));
    
    // Assert data mapped standard text input
    assert!(html.contains("type=\"text\""));
    // Assert currency mapped numeric input
    assert!(html.contains("type=\"number\""));
}
