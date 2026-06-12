use dioxus::prelude::*;
use desk_components::{BiometricSignIn, WorkerDashboard};

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut logged_in_employee = use_signal(|| None::<String>);

    rsx! {
        div { class: "min-h-screen bg-slate-950",
            match &*logged_in_employee.read() {
                Some(emp_id) => rsx! {
                    WorkerDashboard {
                        employee_id: emp_id.clone(),
                        on_logout: move |_| logged_in_employee.set(None)
                    }
                },
                None => rsx! {
                    BiometricSignIn {
                        on_success: move |emp_id| logged_in_employee.set(Some(emp_id)),
                        on_fallback: move |_| println!("Fallback clicked")
                    }
                }
            }
        }
    }
}
