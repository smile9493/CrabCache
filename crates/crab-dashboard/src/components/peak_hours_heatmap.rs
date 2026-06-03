//! 7x24 peak hours heatmap showing request volume by model and hour.

use leptos::prelude::*;

use crate::locale::use_translations;
use crate::types::ModelPeakHourRow;

const HOUR_COUNT: usize = 24;

/// Green → Yellow → Orange → Red based on intensity (0.0–1.0).
/// Green = smooth (low traffic), Red = congested (high traffic).
fn intensity_color(ratio: f64) -> &'static str {
    if ratio <= 0.0 {
        "var(--cc-peak-empty, var(--cc-bg-tertiary))"
    } else if ratio < 0.25 {
        "var(--cc-peak-low, #16a34a)"
    } else if ratio < 0.5 {
        "var(--cc-peak-mid, #eab308)"
    } else if ratio < 0.75 {
        "var(--cc-peak-high, #f97316)"
    } else {
        "var(--cc-peak-max, #ef4444)"
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
    let t = use_translations();
    let day_labels = [
        t.overview_peak_hours_day_mon(),
        t.overview_peak_hours_day_tue(),
        t.overview_peak_hours_day_wed(),
        t.overview_peak_hours_day_thu(),
        t.overview_peak_hours_day_fri(),
        t.overview_peak_hours_day_sat(),
        t.overview_peak_hours_day_sun(),
    ];

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

    // Pre-compute the entire color matrix at component level (7x24) instead of
    // creating 504 individual Memo instances inside the view iteration.
    let color_matrix = Memo::new(move |_| {
        let m = matrix.get();
        let max = max_val.get();
        let mut colors = [[""; HOUR_COUNT]; 7];
        for day in 0..7 {
            for hour in 0..HOUR_COUNT {
                let ratio = m[day][hour] as f64 / max as f64;
                colors[day][hour] = intensity_color(ratio);
            }
        }
        colors
    });

    view! {
        <div class="peak-hours-container">
            <div class="peak-hours-header">
                <h3 class="peak-hours-title">{t.overview_peak_hours_title()}</h3>
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
                        {t.overview_peak_hours_clear_all()}
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
                    {t.overview_peak_hours_total_requests(&format_number(total_requests.get()))}
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
                        {day_labels.iter().map(|label| {
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
                                    let cm = color_matrix;
                                    let m = matrix;
                                    let hc = hovered_cell.clone();
                                    let del = on_delete.clone();
                                    let cmodel = current_model;

                                    view! {
                                        <div
                                            class="peak-hours-cell"
                                            class:hovered=move || hc.get() == Some((day, hour))
                                            style:background-color=move || cm.get()[day][hour].to_string()
                                            on:mouseenter=move |_| hc.set(Some((day, hour)))
                                            on:mouseleave=move |_| hc.set(None)
                                            on:click={
                                                let del = del.clone();
                                                move |_| {
                                                    let model = cmodel.get_untracked();
                                                    let val = m.get()[day][hour];
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
                                                let val = m.get()[day][hour];
                                                t.overview_peak_hours_cell_title(day_labels[day], hour, val)
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
                <span class="peak-hours-legend-label">{t.overview_peak_hours_legend_low()}</span>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-empty, var(--cc-bg-tertiary))"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-low, #16a34a)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-mid, #eab308)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-high, #f97316)"></div>
                <div class="peak-hours-legend-cell" style:background-color="var(--cc-peak-max, #ef4444)"></div>
                <span class="peak-hours-legend-label">{t.overview_peak_hours_legend_high()}</span>
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
