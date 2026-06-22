mod audio;

use std::{collections::HashMap, time::Duration};

use audio::{AudioSink, AudioState};
use gpui::{
    AnyElement, AppContext, Bounds, Context, DragMoveEvent, Empty, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Point, Render, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions, div, layer_shell::*, point,
    prelude::FluentBuilder as _, px, relative, size, transparent_black,
};
use gpui_component::{
    ActiveTheme as _, ElementExt as _, Root, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

const REFRESH_INTERVAL: Duration = Duration::from_millis(800);
const WINDOW_WIDTH: f32 = 390.0;

#[derive(Clone)]
struct VolumeDrag(String);

impl Render for VolumeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct PvctlApp {
    sinks: Vec<AudioSink>,
    slider_bounds: HashMap<String, Bounds<Pixels>>,
    selected_sink_name: Option<String>,
}

impl PvctlApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            sinks: Vec::new(),
            slider_bounds: HashMap::new(),
            selected_sink_name: None,
        };

        if let Ok(state) = audio::load_state() {
            app.apply_audio_state(state);
        }
        app.start_refresh_loop(cx);
        app
    }

    fn start_refresh_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;

                let state = cx.background_spawn(async { audio::load_state() }).await;
                let result = this.update(cx, |this, cx| {
                    if let Ok(state) = state {
                        this.apply_audio_state(state);
                    }
                    cx.notify();
                });

                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_audio_state(&mut self, state: AudioState) {
        self.slider_bounds
            .retain(|name, _| state.sinks.iter().any(|sink| &sink.name == name));

        let mut sinks = state.sinks;
        sinks.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then_with(|| a.description.cmp(&b.description))
        });

        let selected_exists = self
            .selected_sink_name
            .as_ref()
            .is_some_and(|name| sinks.iter().any(|sink| &sink.name == name));
        if !selected_exists {
            self.selected_sink_name = sinks
                .iter()
                .find(|sink| sink.is_default)
                .or_else(|| sinks.first())
                .map(|sink| sink.name.clone());
        }

        self.sinks = sinks;
    }

    fn selected_sink(&self) -> Option<&AudioSink> {
        self.selected_sink_name
            .as_ref()
            .and_then(|name| self.sinks.iter().find(|sink| &sink.name == name))
            .or_else(|| self.sinks.first())
    }

    fn cycle_sink(&mut self, direction: i32, cx: &mut Context<Self>) {
        if self.sinks.len() <= 1 || direction == 0 {
            return;
        }

        let current = self
            .selected_sink_name
            .as_ref()
            .and_then(|name| self.sinks.iter().position(|sink| &sink.name == name))
            .unwrap_or(0);

        let next = if direction > 0 {
            (current + 1) % self.sinks.len()
        } else {
            (current + self.sinks.len() - 1) % self.sinks.len()
        };

        self.selected_sink_name = Some(self.sinks[next].name.clone());
        cx.notify();
    }

    fn refresh_once(&mut self, cx: &mut Context<Self>) {
        cx.notify();

        cx.spawn(async move |this, cx| {
            let state = cx.background_spawn(async { audio::load_state() }).await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(state) = state {
                    this.apply_audio_state(state);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_sink_volume(&mut self, sink_name: String, value: f32, cx: &mut Context<Self>) {
        let percent = value.round().clamp(0.0, 100.0) as u32;

        if let Some(sink) = self.sinks.iter_mut().find(|sink| sink.name == sink_name) {
            sink.volume_percent = percent as f32;
            sink.muted = false;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { audio::set_sink_volume(&sink_name, percent) })
                .await;

            if result.is_err() {
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    fn set_default_sink(&mut self, sink_name: String, cx: &mut Context<Self>) {
        let should_lower_volume = self
            .sinks
            .iter()
            .find(|sink| sink.name == sink_name)
            .is_some_and(|sink| sink.volume_percent > 30.0);

        for sink in &mut self.sinks {
            sink.is_default = sink.name == sink_name;
            if sink.name == sink_name && should_lower_volume {
                sink.volume_percent = 30.0;
            }
        }
        self.selected_sink_name = Some(sink_name.clone());

        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if should_lower_volume {
                        audio::set_sink_volume(&sink_name, 30)?;
                    }
                    audio::set_default_sink(&sink_name)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.refresh_once(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_sink_volume_from_position(
        &mut self,
        sink_name: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.slider_bounds.get(&sink_name).copied() else {
            return;
        };

        let percentage = if bounds.size.width <= px(0.0) {
            0.0
        } else {
            ((position.x - bounds.left()).clamp(px(0.0), bounds.size.width) / bounds.size.width)
                .clamp(0.0, 1.0)
        };

        self.set_sink_volume(sink_name, percentage * 100.0, cx);
    }

    fn render_volume_slider(&self, sink: &AudioSink, cx: &mut Context<Self>) -> AnyElement {
        let percentage = (sink.volume_percent / 100.0).clamp(0.0, 1.0);
        let sink_name_for_bounds = sink.name.clone();
        let sink_name_for_drag = sink.name.clone();
        let entity = cx.weak_entity();
        let bar_color = if sink.muted {
            cx.theme().muted_foreground.opacity(0.55)
        } else {
            cx.theme().primary
        };

        h_flex()
            .h_5()
            .w_full()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h_1p5()
                    .rounded_full()
                    .bg(cx.theme().border.opacity(0.55))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .right(relative(1.0 - percentage))
                            .rounded_full()
                            .bg(bar_color),
                    )
                    .child(
                        div()
                            .id(("volume-thumb", sink.index))
                            .absolute()
                            .top(px(-9.0))
                            .left(relative(percentage))
                            .ml(-px(12.0))
                            .size(px(24.0))
                            .rounded_full()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .shadow_sm()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .when(!sink.muted, |this| {
                                this.on_drag(VolumeDrag(sink_name_for_drag), |drag, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.new(|_| drag.clone())
                                })
                                .on_drag_move(cx.listener(
                                    |this, event: &DragMoveEvent<VolumeDrag>, _window, cx| {
                                        let VolumeDrag(sink_name) = event.drag(cx);
                                        this.set_sink_volume_from_position(
                                            sink_name.clone(),
                                            event.event.position,
                                            cx,
                                        );
                                    },
                                ))
                            }),
                    )
                    .on_prepaint(move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, _| {
                            this.slider_bounds.insert(sink_name_for_bounds, bounds);
                        });
                    }),
            )
            .into_any_element()
    }

    fn render_sink(&self, sink: &AudioSink, cx: &mut Context<Self>) -> AnyElement {
        let volume = sink.volume_percent.round() as u32;
        let default_name = sink.name.clone();
        let status_label = sink_status_label(&sink.state);
        let status_color = if sink.state == "RUNNING" {
            cx.theme().green
        } else {
            cx.theme().muted_foreground
        };
        let volume_color = if sink.muted {
            cx.theme().muted_foreground
        } else {
            cx.theme().foreground
        };

        v_flex()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(if sink.is_default {
                cx.theme().primary.opacity(0.28)
            } else {
                cx.theme().border.opacity(0.72)
            })
            .bg(if sink.is_default {
                cx.theme().primary.opacity(0.08)
            } else {
                cx.theme().popover.opacity(0.72)
            })
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .child(div().size(px(8.0)).rounded_full().bg(if sink.is_default {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground.opacity(0.55)
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .truncate()
                                    .child(short_name(&sink.description)),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(status_color.opacity(0.12))
                                    .text_color(status_color)
                                    .child(status_label),
                            )
                            .children(sink.is_default.then(|| {
                                div()
                                    .text_xs()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(cx.theme().primary.opacity(0.16))
                                    .text_color(cx.theme().primary)
                                    .child("默认")
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .w_10()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(volume_color)
                                    .child(format!("{volume}%")),
                            )
                            .children((!sink.is_default).then(|| {
                                Button::new(format!("default-{}", sink.index))
                                    .xsmall()
                                    .secondary()
                                    .label("设为默认")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_default_sink(default_name.clone(), cx);
                                    }))
                            })),
                    ),
            )
            .child(self.render_volume_slider(sink, cx))
            .into_any_element()
    }
}

impl Render for PvctlApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .id("device-panel")
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .bottom(px(8.0))
                    .left(px(8.0))
                    .gap_2()
                    .p_2()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .bg(cx.theme().background.opacity(0.92))
                    .shadow_lg()
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        let delta = event.delta.pixel_delta(window.line_height());
                        let amount = if delta.y.abs() >= delta.x.abs() {
                            delta.y
                        } else {
                            delta.x
                        };

                        if amount < px(0.0) {
                            this.cycle_sink(1, cx);
                            cx.stop_propagation();
                        } else if amount > px(0.0) {
                            this.cycle_sink(-1, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .when(self.sinks.is_empty(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("没有输出设备"),
                        )
                    })
                    .children(self.selected_sink().map(|sink| self.render_sink(sink, cx))),
            )
    }
}

fn desired_window_size() -> gpui::Size<gpui::Pixels> {
    size(px(WINDOW_WIDTH), px(102.0))
}

fn sink_status_label(state: &str) -> SharedString {
    match state {
        "RUNNING" => "正在工作",
        "IDLE" => "空闲",
        "SUSPENDED" => "待机",
        "UNKNOWN" => "未知",
        other => other,
    }
    .into()
}

fn short_name(description: &str) -> SharedString {
    description
        .replace(" Analog Stereo", "")
        .replace(" Digital Stereo (HDMI)", " HDMI")
        .into()
}

fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.0), px(0.0)),
                size: desired_window_size(),
            })),
            app_id: Some("pvctl".to_string()),
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: Some(WindowDecorations::Client),
            focus: false,
            is_resizable: false,
            is_minimizable: false,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "pvctl".to_string(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::RIGHT,
                margin: Some((px(16.0), px(16.0), px(0.0), px(0.0))),
                keyboard_interactivity: KeyboardInteractivity::None,
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(window_options, |window, cx| {
            let view = cx.new(PvctlApp::new);
            cx.new(|cx| {
                Root::new(view, window, cx)
                    .bordered(false)
                    .bg(transparent_black())
            })
        })
        .expect("failed to open pvctl window");
    });
}
