use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct BiometricSignInProps {
    pub on_success: EventHandler<String>, // Returns employee ID on success
    pub on_fallback: EventHandler<()>,
}

#[component]
pub fn BiometricSignIn(props: BiometricSignInProps) -> Element {
    let mut status = use_signal(|| "Ready to authenticate".to_string());
    let mut is_scanning = use_signal(|| false);
    let mut pin_input = use_signal(|| String::new());
    let mut show_pin_fallback = use_signal(|| false);

    let handle_biometric_trigger = move |_| {
        is_scanning.set(true);
        status.set("Scanning fingerprint/face...".to_string());

        // Simulate WebAuthn biometric delay
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            is_scanning.set(false);
            status.set("Biometric scan successful!".to_string());
            // Trigger parent callback
            props.on_success.call("EMP-001".to_string());
        });
    };

    let handle_pin_submit = move |_| {
        if pin_input.read().len() >= 4 {
            status.set("PIN accepted!".to_string());
            props.on_success.call("EMP-001".to_string());
        } else {
            status.set("PIN must be at least 4 digits".to_string());
        }
    };

    rsx! {
        div { 
            class: "min-h-screen flex flex-col justify-between bg-slate-900 text-white p-6 font-sans md:max-w-md md:mx-auto md:shadow-2xl md:rounded-3xl",
            
            // Top Bar
            div { class: "flex justify-between items-center pt-4",
                span { class: "text-sm text-slate-400 font-medium", "Factory OS Mobile" }
                span { class: "px-2.5 py-1 bg-green-500/10 text-green-400 text-xs font-semibold rounded-full border border-green-500/20", "Connected" }
            }

            // Header Section
            div { class: "text-center my-auto py-8 space-y-2",
                h1 { class: "text-3xl font-extrabold tracking-tight text-white", "Worker Sign-In" }
                p { class: "text-sm text-slate-400", "Place your finger on the sensor or use FaceID" }
            }

            // Center Scanner / Status
            div { class: "flex flex-col items-center justify-center space-y-6 my-auto",
                // Animated Biometric Icon Ring
                div { 
                    class: format!(
                        "relative flex items-center justify-center w-28 h-28 rounded-full border-4 transition-all duration-500 {}",
                        if *is_scanning.read() { "border-blue-500 bg-blue-500/10 animate-pulseScale" } 
                        else if status.read().contains("successful") { "border-green-500 bg-green-500/10" } 
                        else { "border-slate-700 bg-slate-800 hover:border-slate-500 cursor-pointer" }
                    ),
                    onclick: handle_biometric_trigger,
                    
                    // Biometric Scanning SVG Icon
                    svg { 
                        class: format!("w-14 h-14 {}", if *is_scanning.read() { "text-blue-400" } else { "text-slate-300" }),
                        fill: "none", 
                        view_box: "0 0 24 24", 
                        stroke: "currentColor", 
                        stroke_width: "1.5",
                        path { 
                            stroke_linecap: "round", 
                            stroke_linejoin: "round", 
                            d: "M12 11c0 3.517-1.009 6.799-2.753 9.571m-3.44-2.04l.054-.09A13.916 13.916 0 009 11a13.917 13.917 0 00-2.338-7.797M5.625 20H4.75a2.25 2.25 0 01-2.25-2.25v-1.383c0-.973.396-1.906 1.098-2.6a13.907 13.907 0 0110.962-4.113m.012 0a13.9 13.9 0 013.076 1.411m0 0a13.9 13.9 0 00-3.076-1.411m3.076 1.411V9M12 3v1m6.364.364l-.707.707M12 21a9 9 0 100-18 9 9 0 000 18z"
                        }
                    }
                }

                // Status message
                p { 
                    class: format!(
                        "text-base font-semibold text-center transition-all duration-300 {}",
                        if *is_scanning.read() { "text-blue-400 animate-pulse" } 
                        else if status.read().contains("successful") { "text-green-400" } 
                        else { "text-slate-300" }
                    ),
                    "{status}"
                }
            }

            // PIN Fallback Section
            if *show_pin_fallback.read() {
                div { class: "mt-4 p-4 bg-slate-800 rounded-2xl border border-slate-700 space-y-3",
                    label { class: "block text-xs font-bold uppercase tracking-wider text-slate-400", "Enter Security PIN" }
                    div { class: "flex space-x-2",
                        input { 
                            class: "flex-1 border-0 bg-slate-900 rounded-xl p-3 text-center text-xl font-bold tracking-widest text-white focus:ring-2 focus:ring-blue-500 outline-none",
                            r#type: "password",
                            maxlength: "6",
                            placeholder: "••••",
                            value: "{pin_input}",
                            oninput: move |evt| pin_input.set(evt.value())
                        }
                        button { 
                            class: "bg-blue-600 hover:bg-blue-500 font-bold px-5 rounded-xl transition-all active:scale-95",
                            onclick: handle_pin_submit,
                            "Verify"
                        }
                    }
                }
            }

            // Bottom Actions (Mobile layout footer)
            div { class: "space-y-3 pb-4",
                if !*show_pin_fallback.read() {
                    button { 
                        class: "w-full py-4 bg-slate-800 hover:bg-slate-700 text-slate-300 font-bold rounded-2xl border border-slate-700 transition-all active:scale-98 text-sm",
                        onclick: move |_| show_pin_fallback.set(true),
                        "Use Security PIN"
                    }
                } else {
                    button { 
                        class: "w-full py-4 bg-transparent hover:bg-slate-800 text-slate-400 font-semibold rounded-2xl transition-all active:scale-98 text-sm",
                        onclick: move |_| show_pin_fallback.set(false),
                        "Cancel PIN Entry"
                    }
                }
            }
        }
    }
}
