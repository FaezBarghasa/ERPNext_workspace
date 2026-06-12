use desk_components::{DynamicFormInterpreter, FormProps};
use dioxus::prelude::*;

#[test]
fn test_interpreter_compilation_and_props() {
    // Verify props construction and default initialization compiles correctly
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

    // Test that constructing the VirtualDom does not panic
    let mut vdom = VirtualDom::new_with_props(DynamicFormInterpreter, props);
    let _ = vdom.rebuild_in_place();
}
