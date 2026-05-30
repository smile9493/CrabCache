//! 7x24 peak hours heatmap showing request volume by model and hour.

use leptos::prelude::*;

use crate::types::ModelPeakHourRow;

const DAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const HOUR_COUNT: usize = 24;

/// Interpolate between empty and peak colors based on intensity (0.0-1.0).
fn intensity_color(ratio: f64) -> &'static str {
    if ratio <= 0.0 {
        "var(--cc-peak-empty, var(--cc-bg-tertiary))"
    } else if ratio < 0.25 {
        "var(--cc-peak-low, #1e3a5f)"
    } else if ratio < 0.5 {
        "var(--cc-peak-mid, #2563eb)"
    } else if ratio < 0.75 {
        "var(--cc-peak-high, #60a5fa)"
    } else {
        "var(--cc-peak-max, #93c5fd)"
    }
}

/// Convert a unix-ms hour_bucket to local-time (weekday 0-6 = Mon-Sun, hour 0-23).
fn bucket_to_day_hour(bucket_ms: i64) -> (usize, usize) {
    // Use JS Date to get local timezone weekday/hour.
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(bucket_ms as f64));
    // JS getDay(): 0=Sun,1=Mon,...,6=Sat → convert to Mon=0,...,Sun=6
    let weekday = ((date.get_day() + 6) % 7) as usize;
    let hour = date.get_hours() as usize;
    (weekday, hour)
}

/// Reverse: given a local weekday (Mon=0) and hour, find matching hour_buckets in data.
fn find_bucket_for_cell(
    data: &[ModelPeakHourRow],
    model: &str,
    target_day: usize,
    target_hour: usize,
) -> Vec<i64> {
    data.iter()
        .filter(|r| r.model == model)
        .filter(|r| {
            let (d, h) = bucket_to_day_hour(r.hour_bucket);
            d == target_day && h == target_hour
        })
        .map(|r| r.hour_bucket)
        .collect()
}

/// Build a 7x24 matrix from peak hour rows for a given model.
fn build_matrix(data: &[ModelPeakHourRow], model: &str) -> [[u64; HOUR_COUNT]; 7] {
    let mut matrix = [[0u64; HOUR_COUNT]; 7];
    for row in data.iter().filter(|r| r.model == model) {
        let (day, hour) = bucket_to_day_hour(row.hour_bucket);
        if day < 7 && hour < HOUR_COUNT {
            matrix[day][hour] += row.request_count;
        }
    }
    matrix
}

/// Compute max value in matrix for color scaling.
fn matrix_max(matrix: &[[u64; HOUR_COUNT]; 7]) -> u64 {
    matrix
        .iter()
        .flat_map(|row| row.iter())
        .copied()
        .max()
        .unwrap_or(1)
        .max(1)
}

#[component]
pub fn PeakHoursHeatmap(
    data: Signal<Vec<ModelPeakHourRow>>,
    models: Signal<Vec<String>>,
    on_delete: Callback<(String, i64)>,
) -> impl IntoView {
    let selected_model: RwSignal<usize> = RwSignal::new(0);
    let hovered_cell: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);

    let current_model = Memo::new(move |_| {
        let m = models.get();
        m.get(selected_model.get()).cloned().unwrap_or_default()
    });

    let matrix = Memo::new(move |_| {
        let m = current_model.get();
        let d = data.get();
        build_matrix(&d, &m)
    });

    let max_val = Memo::new(move |_| matrix_max(&matrix.get()));

    let total_requests = Memo::new(move |_| {
        let m = current_model.get();
        data.get()
            .iter()
            .filter(|r| r.model == m)
            .map(|r| r.request_count)
            .sum::<u64>()
    });

    view! {
        <div class="peak-hours-container">
            <div class="peak-hours-header">
                <h3 class="peak-hours-title">"Model Peak Hours"</h3>
                <div class="peak-hours-actions">
                    <button
                        class="btn btn-sm btn-ghost"
                        on:click={
                            let on_delete = on_delete.clone();
                            move |_| {
                                let m = current_model.get_untracked();
                                if !m.is_empty() {
                                    on_delete.run((m, 0));
                                }
                            }
                        }
                    >
                        "Delete All"
                    </button>
                </div>
            </div>

            // Model tabs
            <div class="peak-hours-model-tabs">
                {move || {
                    let m = models.get();
                    let sel = selected_model.get();
                    m.into_iter().enumerate().map(|(i, model_name)| {
                        let is_active = i == sel;
                        let name = model_name.clone();
                        view! {
                            <button
                                class=if is_active { "peak-hours-tab active" } else { "peak-hours-tab" }
                                on:click=move |_| selected_model.set(i)
                            >
                                {name}
                            </button>
                        }
                    }).collect_view()
                }}
            </div>

            // Stats row
            <div class="peak-hours-stats">
                <span class="peak-hours-stat">
                    "Total: " {move || format_number(total_requests.get())} " requests"
                </span>
            </div>

            // Heatmap grid
            <div class="peak-hours-grid-wrapper">
                // Hour labels (left axis)
                <div class="peak-hours-hour-labels">
                    {(0..HOUR_COUNT).map(|h| {
                        view! {
                            <span class="peak-hours-hour-label">
                                {format!("{:02}", h)}
                            </span>
                        }
                    }).collect_view()}
                </div>

                // Grid
                <div class="peak-hours-grid">
                    // Day headers
                    <div class="peak-hours-day-header-row">
                        {DAY_LABELS.iter().map(|label| {
                            view! {
                                <span class="peak-hours-day-header">{*label}</span>
                            }
                        }).collect_view()}
                    </div>

                    // Rows (one per hour)
                    {(0..HOUR_COUNT).map(|hour| {
                        view! {
                            <div class="peak-hours-row">
                                {(0..7usize).map(|day| {
                                    let m = matrix;
                                    let mx = max_val;
                                    let hc = hovered_cell.clone();
                                    let del = on_delete.clone();
                                    let cm = current_model;
                                    let bg_color = Memo::new(move |_| {
                                        let val = m.get()[day][hour];
                                        let max = mx.get();
                                        let ratio = val as f64 / max as f64;
                                        intensity_color(ratio)
                                    });
                                    let val_text = Memo::new(move |_| {
                                        m.get()[day][hour]
                                    });
                                    let is_hovered = Memo::new(move |_| {
                                        hc.get() == Some((day, hour))
                                    });

                                    view! {
                                        <div
                                            class=move || {
                                                if is_hovered.get() {
                                                    "peak-hours-cell hovered"
                                                } else {
                                                    "peak-hours-cell"
                                                }
                                            }
                                            style:background-color=move || bg_color.get()
                                            on:mouseenter=move |_| hc.set(Some((day, hour)))
                                            on:mouseleave=move |_| hc.set(None)
                                            on:click={
                                                let del = del.clone();
                                                move |_| {
                                                    let model = cm.get_untracked();
                                                    let val = val_text.get_untracked();
                                                    if !model.is_empty() && val > 0 {
                                                        // Find actual bucket from data for this cell
                                                        let buckets = find_bucket_for_cell(
                                                            &data.get_untracked(), &model, day, hour,
                                                        );
                                                        for bucket in buckets {
                                                            del.run((model.clone(), bucket));
                                                        }
                                                    }
                                                }
                                            }
                                            title=move || {
                                                let val = val_text.get();
                                                format!("{} {:02}:00 — {} requests", DAY_LABELS[day], hour, val)
                                            }
                                        />
                                    }
                                }).collect_view()}
                            </div>
                        }
                    }).collect_view()}
                </div>
            </div>

            // Legend
            <div class="peak-hours-legend">
                <span class="peak-hours-legend-label">"Less"</span>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-empty, var(--cc-bg-tertiary))"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-low, #1e3a5f)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-mid, #2563eb)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-high, #60a5fa)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-max, #93c5fd)"></div>
                <span class="peak-hours-legend-label">"More"</span>
            </div>
        </div>
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}
