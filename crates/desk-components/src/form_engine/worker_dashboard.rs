use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct WorkerDashboardProps {
    pub employee_id: String,
    pub on_logout: EventHandler<()>,
}

#[component]
pub fn WorkerDashboard(props: WorkerDashboardProps) -> Element {
    let mut status_msg = use_signal(|| "Ready".to_string());
    let mut is_loading = use_signal(|| false);
    let productivity = use_signal(|| 95.8);
    let mut is_clocked_in = use_signal(|| false);
    let mut last_clock_in = use_signal(|| "--:--".to_string());
    
    // GPS Geolocation mock coordinates (representing factory geofencing area)
    let factory_lat = use_signal(|| "37.7749".to_string());
    let factory_lon = use_signal(|| "-122.4194".to_string());
    let mut worker_lat = use_signal(|| "37.7749".to_string()); // default inside geofence
    let mut worker_lon = use_signal(|| "-122.4194".to_string());
    
    // CMS Video Embed Tester
    let mut video_input_url = use_signal(|| "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string());
    let mut embed_html = use_signal(|| "".to_string());

    let handle_clock_in_out = move |_| {
        is_loading.set(true);
        status_msg.set("Verifying biometric GPS boundary...".to_string());

        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            
            // Validate geofence coordinates
            let w_lat: f64 = worker_lat.read().parse().unwrap_or(0.0);
            let w_lon: f64 = worker_lon.read().parse().unwrap_or(0.0);
            let f_lat: f64 = factory_lat.read().parse().unwrap_or(0.0);
            let f_lon: f64 = factory_lon.read().parse().unwrap_or(0.0);
            
            // Simple distance check: offset absolute difference
            let diff_lat = (w_lat - f_lat).abs();
            let diff_lon = (w_lon - f_lon).abs();
            
            if diff_lat > 0.01 || diff_lon > 0.01 {
                status_msg.set("Geofence Check Failed! Outside factory boundary.".to_string());
            } else {
                let clocked = !*is_clocked_in.read();
                is_clocked_in.set(clocked);
                if clocked {
                    last_clock_in.set("08:00 AM".to_string());
                    status_msg.set("Clock-in recorded successfully!".to_string());
                } else {
                    status_msg.set("Clock-out recorded successfully!".to_string());
                }
            }
            is_loading.set(false);
        });
    };

    let handle_video_embed_request = move |_| {
        spawn(async move {
            let url = video_input_url.read().clone();
            // Call simulated embedding generator from CMS
            if url.contains("youtube.com") || url.contains("youtu.be") {
                embed_html.set(
                    r#"<div class="video-container" style="position:relative;padding-bottom:56.25%;height:0;overflow:hidden;"><iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ" frameborder="0" allowfullscreen style="position:absolute;top:0;left:0;width:100%;height:100%;"></iframe></div>"#.to_string()
                );
            } else {
                embed_html.set("<p class='text-red-400'>Unsupported or unparsed media URL</p>".to_string());
            }
        });
    };

    rsx! {
        div { 
            class: "min-h-screen bg-slate-950 text-white font-sans pb-12",
            
            // Header bar
            header { class: "bg-slate-900 border-b border-slate-800 px-6 py-4 flex justify-between items-center sticky top-0 z-50",
                div { class: "flex items-center space-x-3",
                    div { class: "w-8 h-8 rounded-lg bg-blue-600 flex items-center justify-center font-bold text-white", "F" }
                    h1 { class: "text-lg font-bold tracking-tight", "Factory OS Dashboard" }
                }
                div { class: "flex items-center space-x-4",
                    span { class: "hidden md:inline text-sm text-slate-400", "Worker: {props.employee_id}" }
                    button { 
                        class: "text-xs font-semibold px-4 py-2 bg-slate-800 hover:bg-slate-700 rounded-xl transition border border-slate-700",
                        onclick: move |_| props.on_logout.call(()),
                        "Logout"
                    }
                }
            }

            // Main Layout Container (Responsive: Grid on desktop, stacked on mobile)
            div { class: "max-w-7xl mx-auto px-4 md:px-6 py-6 grid grid-cols-1 lg:grid-cols-3 gap-6",
                
                // Left Column: Shift details & Geofenced Clock-in
                div { class: "lg:col-span-2 space-y-6",
                    
                    // Welcome & Core Action Card
                    div { class: "p-6 bg-slate-900 rounded-3xl border border-slate-800 shadow-xl space-y-6",
                        div { class: "flex justify-between items-start",
                            div {
                                h2 { class: "text-xl font-bold", "Today's Work Shift" }
                                p { class: "text-sm text-slate-400 mt-1", "Standard factory shift schedule: 08:00 - 17:00" }
                            }
                            span { 
                                class: format!(
                                    "px-3 py-1 rounded-full text-xs font-bold uppercase tracking-wider border {}",
                                    if *is_clocked_in.read() { "bg-green-500/10 text-green-400 border-green-500/20" } 
                                    else { "bg-yellow-500/10 text-yellow-400 border-yellow-500/20" }
                                ),
                                if *is_clocked_in.read() { "Active Shift" } else { "Off Duty" }
                            }
                        }

                        // Shift Stats grid
                        div { class: "grid grid-cols-2 md:grid-cols-3 gap-4 py-2",
                            div { class: "p-4 bg-slate-950 rounded-2xl border border-slate-800/60",
                                span { class: "text-xs text-slate-500 uppercase tracking-wider block", "Clock In Time" }
                                span { class: "text-lg font-bold block mt-1", "{last_clock_in}" }
                            }
                            div { class: "p-4 bg-slate-950 rounded-2xl border border-slate-800/60",
                                span { class: "text-xs text-slate-500 uppercase tracking-wider block", "Total Duration" }
                                span { class: "text-lg font-bold block mt-1", if *is_clocked_in.read() { "5h 12m" } else { "--:--" } }
                            }
                            div { class: "p-4 bg-slate-950 rounded-2xl border border-slate-800/60 col-span-2 md:col-span-1",
                                span { class: "text-xs text-slate-500 uppercase tracking-wider block", "Productivity Score" }
                                span { class: "text-lg font-bold text-blue-400 block mt-1", "{productivity}%" }
                            }
                        }

                        // Geofence Coordinate Adjustment (Touch/Simulate coordinates on mobile)
                        div { class: "space-y-3 p-4 bg-slate-950 rounded-2xl border border-slate-800/60",
                            span { class: "text-xs font-bold text-slate-400 uppercase tracking-wider block", "Biometric Geolocation Simulator" }
                            div { class: "grid grid-cols-2 gap-3",
                                div { class: "space-y-1",
                                    label { class: "text-[10px] text-slate-500 uppercase font-semibold", "Worker Latitude" }
                                    input { 
                                        class: "w-full bg-slate-900 border border-slate-800 rounded-xl p-2.5 text-sm outline-none text-white focus:border-blue-500",
                                        value: "{worker_lat}",
                                        oninput: move |evt| worker_lat.set(evt.value())
                                    }
                                }
                                div { class: "space-y-1",
                                    label { class: "text-[10px] text-slate-500 uppercase font-semibold", "Worker Longitude" }
                                    input { 
                                        class: "w-full bg-slate-900 border border-slate-800 rounded-xl p-2.5 text-sm outline-none text-white focus:border-blue-500",
                                        value: "{worker_lon}",
                                        oninput: move |evt| worker_lon.set(evt.value())
                                    }
                                }
                            }
                            p { class: "text-[11px] text-slate-500", "Factory center at: {factory_lat}, {factory_lon}. Increase coordinates to simulate clock-in failures." }
                        }

                        // Large Call to Action Button
                        div { class: "pt-2 space-y-3",
                            button { 
                                class: format!(
                                    "w-full py-4 text-base font-bold rounded-2xl transition-all duration-300 shadow-lg active:scale-98 flex items-center justify-center space-x-2 text-white {}",
                                    if *is_loading.read() { "bg-blue-600/50 cursor-not-allowed" } 
                                    else if *is_clocked_in.read() { "bg-red-600 hover:bg-red-500" } 
                                    else { "bg-blue-600 hover:bg-blue-500" }
                                ),
                                disabled: *is_loading.read(),
                                onclick: handle_clock_in_out,
                                if *is_loading.read() { "Verifying..." }
                                else if *is_clocked_in.read() { "Clock Out" }
                                else { "Clock In" }
                            }
                            
                            p { class: "text-center text-xs text-slate-400 mt-2 font-medium", "Status: {status_msg}" }
                        }
                    }

                    // CMS Universal Video Player Preview Widget
                    div { class: "p-6 bg-slate-900 rounded-3xl border border-slate-800 space-y-4",
                        h3 { class: "text-lg font-bold", "ERP CMS Training Videos & Manuals" }
                        p { class: "text-sm text-slate-400", "Paste any YouTube, Vimeo, or MP4 URL to view interactive training manuals." }
                        
                        div { class: "flex flex-col md:flex-row space-y-2 md:space-y-0 md:space-x-2",
                            input { 
                                class: "flex-1 bg-slate-950 border border-slate-800 rounded-xl p-3 text-sm text-white placeholder-slate-500 outline-none focus:border-blue-500",
                                placeholder: "Enter media streaming URL...",
                                value: "{video_input_url}",
                                oninput: move |evt| video_input_url.set(evt.value())
                            }
                            button { 
                                class: "bg-blue-600 hover:bg-blue-500 text-sm font-bold py-3 px-6 rounded-xl transition active:scale-95 whitespace-nowrap",
                                onclick: handle_video_embed_request,
                                "Load Embed"
                            }
                        }

                        // Responsive embed rendering container
                        if !embed_html.read().is_empty() {
                            div { 
                                class: "mt-4 rounded-2xl overflow-hidden border border-slate-800 bg-slate-950",
                                dangerous_inner_html: "{embed_html}" 
                            }
                        }
                    }
                }

                // Right Column: Shift timeline & logs
                div { class: "space-y-6",
                    
                    // Live Feed alerts
                    div { class: "p-6 bg-slate-900 rounded-3xl border border-slate-800 space-y-4",
                        h3 { class: "text-lg font-bold", "Shift Live Notifications" }
                        
                        div { class: "space-y-3",
                            div { class: "p-3.5 bg-slate-950 rounded-2xl border border-slate-800/80 flex items-start space-x-3",
                                div { class: "w-2 h-2 rounded-full bg-blue-500 mt-1.5" }
                                div {
                                    span { class: "text-xs font-semibold text-slate-300 block", "Safety Instruction Manual Updated" }
                                    span { class: "text-[10px] text-slate-500", "10 minutes ago" }
                                }
                            }
                            div { class: "p-3.5 bg-slate-950 rounded-2xl border border-slate-800/80 flex items-start space-x-3",
                                div { class: "w-2 h-2 rounded-full bg-green-500 mt-1.5" }
                                div {
                                    span { class: "text-xs font-semibold text-slate-300 block", "Biometric Clock-in Verified (Inside Area)" }
                                    span { class: "text-[10px] text-slate-500", "08:02 AM" }
                                }
                            }
                            div { class: "p-3.5 bg-slate-950 rounded-2xl border border-slate-800/80 flex items-start space-x-3",
                                div { class: "w-2 h-2 rounded-full bg-slate-600 mt-1.5" }
                                div {
                                    span { class: "text-xs font-semibold text-slate-400 block", "Weekly Payroll Batch post completed" }
                                    span { class: "text-[10px] text-slate-500", "Yesterday" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
