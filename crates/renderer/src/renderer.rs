use std::path::PathBuf;

use dioxus_core::VirtualDom;
use freya_common::AccessibilityFocusStrategy;
use freya_core::{
    dom::SafeDOM,
    events::{
        EventName,
        PlatformEvent,
        PlatformEventData,
    },
    prelude::{
        EventMessage,
        NavigationMode,
    },
};
use freya_elements::events::{
    map_winit_key,
    map_winit_modifiers,
    map_winit_physical_key,
    Code,
    Key,
};
use torin::geometry::CursorPoint;
use winit::{
    application::ApplicationHandler,
    event::{
        ElementState,
        Ime,
        KeyEvent,
        MouseButton,
        MouseScrollDelta,
        StartCause,
        Touch,
        TouchPhase,
        WindowEvent,
    },
    event_loop::{
        EventLoop,
        EventLoopProxy,
    },
    keyboard::ModifiersState,
};

use crate::{
    devtools::Devtools,
    window_state::{
        CreatedState,
        NotCreatedState,
        WindowState,
    },
    HoveredNode,
    LaunchConfig,
};

const WHEEL_SPEED_MODIFIER: f64 = 53.0;
const TOUCHPAD_SPEED_MODIFIER: f64 = 2.0;

/// Desktop renderer using Skia, Glutin and Winit
pub struct DesktopRenderer<'a, State: Clone + 'static> {
    pub(crate) event_loop_proxy: EventLoopProxy<EventMessage>,
    pub(crate) state: WindowState<'a, State>,
    pub(crate) hovered_node: HoveredNode,
    pub(crate) cursor_pos: CursorPoint,
    pub(crate) mouse_state: ElementState,
    pub(crate) modifiers_state: ModifiersState,
    pub(crate) dropped_file_path: Option<PathBuf>,
    pub(crate) custom_scale_factor: f64,
}

impl<'a, State: Clone + 'static> DesktopRenderer<'a, State> {
    /// Run the Desktop Renderer.
    pub fn launch(
        vdom: VirtualDom,
        sdom: SafeDOM,
        mut config: LaunchConfig<State>,
        devtools: Option<Devtools>,
        hovered_node: HoveredNode,
    ) {
        let mut event_loop_builder = EventLoop::<EventMessage>::with_user_event();
        let event_loop_builder_hook = config.window_config.event_loop_builder_hook.take();
        if let Some(event_loop_builder_hook) = event_loop_builder_hook {
            event_loop_builder_hook(&mut event_loop_builder);
        }
        let event_loop = event_loop_builder
            .build()
            .expect("Failed to create event loop.");
        let proxy = event_loop.create_proxy();

        let mut desktop_renderer =
            DesktopRenderer::new(vdom, sdom, config, devtools, hovered_node, proxy);

        event_loop.run_app(&mut desktop_renderer).unwrap();
    }

    pub fn new(
        vdom: VirtualDom,
        sdom: SafeDOM,
        config: LaunchConfig<'a, State>,
        devtools: Option<Devtools>,
        hovered_node: HoveredNode,
        proxy: EventLoopProxy<EventMessage>,
    ) -> Self {
        DesktopRenderer {
            state: WindowState::NotCreated(NotCreatedState {
                sdom,
                devtools,
                vdom,
                config,
            }),
            hovered_node,
            event_loop_proxy: proxy,
            cursor_pos: CursorPoint::default(),
            mouse_state: ElementState::Released,
            modifiers_state: ModifiersState::default(),
            dropped_file_path: None,
            custom_scale_factor: 0.,
        }
    }

    // Send and process an event
    fn send_event(&mut self, event: PlatformEvent) {
        let scale_factor = self.scale_factor();
        self.state
            .created_state()
            .app
            .send_event(event, scale_factor);
    }

    /// Get the current scale factor of the Window
    fn scale_factor(&self) -> f64 {
        match &self.state {
            WindowState::Created(CreatedState { window, .. }) => {
                window.scale_factor() + self.custom_scale_factor
            }
            _ => 0.0,
        }
    }

    /// Run the `on_setup` callback that was passed to the launch function
    pub fn run_on_setup(&mut self) {
        let state = self.state.created_state();
        if let Some(on_setup) = state.window_config.on_setup.take() {
            (on_setup)(&mut state.window)
        }
    }

    /// Run the `on_exit` callback that was passed to the launch function
    pub fn run_on_exit(&mut self) {
        let state = self.state.created_state();
        if let Some(on_exit) = state.window_config.on_exit.take() {
            (on_exit)(&mut state.window)
        }
    }
}

impl<'a, State: Clone> ApplicationHandler<EventMessage> for DesktopRenderer<'a, State> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.state.has_been_created() {
            self.state.create(event_loop, &self.event_loop_proxy);
            self.run_on_setup();
        }
    }

    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if cause == StartCause::Init {
            self.event_loop_proxy
                .send_event(EventMessage::PollVDOM)
                .ok();
        }
    }

    fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: EventMessage) {
        let scale_factor = self.scale_factor();
        let CreatedState { window, app, .. } = self.state.created_state();
        match event {
            EventMessage::FocusAccessibilityNode(strategy) => {
                app.request_focus_node(strategy);
                window.request_redraw();
            }
            EventMessage::RequestRerender => {
                window.request_redraw();
            }
            EventMessage::RequestFullRerender => {
                app.resize(window);
                window.request_redraw();
            }
            EventMessage::InvalidateArea(mut area) => {
                let fdom = app.sdom.get();
                area.size *= scale_factor as f32;
                let mut compositor_dirty_area = fdom.compositor_dirty_area();
                compositor_dirty_area.unite_or_insert(&area)
            }
            EventMessage::RemeasureTextGroup(text_id) => {
                app.measure_text_group(text_id, scale_factor);
            }
            EventMessage::Accessibility(accesskit_winit::WindowEvent::ActionRequested(request)) => {
                if accesskit::Action::Focus == request.action {
                    app.request_focus_node(AccessibilityFocusStrategy::Node(request.target));
                    window.request_redraw();
                }
            }
            EventMessage::Accessibility(accesskit_winit::WindowEvent::InitialTreeRequested) => {
                app.init_accessibility_on_next_render = true;
            }
            EventMessage::SetCursorIcon(icon) => window.set_cursor(icon),
            EventMessage::WithWindow(use_window) => (use_window)(window),
            EventMessage::ExitApp => event_loop.exit(),
            EventMessage::PlatformEvent(platform_event) => self.send_event(platform_event),
            EventMessage::PollVDOM => {
                app.poll_vdom(window);
            }
            _ => {}
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let scale_factor = self.scale_factor();
        let CreatedState {
            surface,
            dirty_surface,
            window,
            window_config,
            app,
            is_window_focused,
            graphics_driver,
            ..
        } = self.state.created_state();
        app.accessibility
            .process_accessibility_event(&event, window);
        match event {
            WindowEvent::ThemeChanged(theme) => {
                app.platform_sender.send_modify(|state| {
                    state.preferred_theme = theme.into();
                });
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.send_event(PlatformEvent {
                    name: EventName::KeyDown,
                    data: PlatformEventData::Keyboard {
                        key: Key::Character(text),
                        code: Code::Unidentified,
                        modifiers: map_winit_modifiers(self.modifiers_state),
                    },
                });
            }
            WindowEvent::RedrawRequested => {
                app.platform_sender.send_if_modified(|state| {
                    let scale_factor_is_different = state.scale_factor == scale_factor;
                    state.scale_factor = scale_factor;
                    scale_factor_is_different
                });

                if app.process_layout_on_next_render {
                    app.process_layout(window.inner_size(), scale_factor);

                    app.process_layout_on_next_render = false;
                }

                if app.process_accessibility_on_next_render {
                    app.process_accessibility(window);
                }

                if app.init_accessibility_on_next_render {
                    app.init_accessibility();
                    app.init_accessibility_on_next_render = false;
                }

                graphics_driver.make_current();

                app.render(
                    &self.hovered_node,
                    window_config.background,
                    surface,
                    dirty_surface,
                    window,
                    scale_factor,
                );

                app.event_loop_tick();
                window.pre_present_notify();
                graphics_driver.flush_and_submit();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                app.set_navigation_mode(NavigationMode::NotKeyboard);

                self.mouse_state = state;

                let name = match state {
                    ElementState::Pressed => EventName::MouseDown,
                    ElementState::Released => match button {
                        MouseButton::Middle => EventName::MiddleClick,
                        MouseButton::Right => EventName::RightClick,
                        MouseButton::Left => EventName::MouseUp,
                        _ => EventName::PointerUp,
                    },
                };

                self.send_event(PlatformEvent {
                    name,
                    data: PlatformEventData::Mouse {
                        cursor: self.cursor_pos,
                        button: Some(button),
                    },
                });
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                if TouchPhase::Moved == phase {
                    let scroll_data = {
                        match delta {
                            MouseScrollDelta::LineDelta(x, y) => (
                                (x as f64 * WHEEL_SPEED_MODIFIER),
                                (y as f64 * WHEEL_SPEED_MODIFIER),
                            ),
                            MouseScrollDelta::PixelDelta(pos) => (
                                (pos.x * TOUCHPAD_SPEED_MODIFIER),
                                (pos.y * TOUCHPAD_SPEED_MODIFIER),
                            ),
                        }
                    };

                    self.send_event(PlatformEvent {
                        name: EventName::Wheel,
                        data: PlatformEventData::Wheel {
                            scroll: CursorPoint::from(scroll_data),
                            cursor: self.cursor_pos,
                        },
                    });
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers_state = modifiers.state();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        logical_key,
                        state,
                        ..
                    },
                ..
            } => {
                if !*is_window_focused {
                    return;
                }

                #[cfg(not(feature = "disable-zoom-shortcuts"))]
                {
                    let is_control_pressed = {
                        if cfg!(target_os = "macos") {
                            self.modifiers_state.super_key()
                        } else {
                            self.modifiers_state.control_key()
                        }
                    };

                    if is_control_pressed && state == ElementState::Pressed {
                        let ch = logical_key.to_text();
                        let render_with_new_scale_factor = if ch == Some("+") {
                            self.custom_scale_factor =
                                (self.custom_scale_factor + 0.10).clamp(-1.0, 5.0);
                            true
                        } else if ch == Some("-") {
                            self.custom_scale_factor =
                                (self.custom_scale_factor - 0.10).clamp(-1.0, 5.0);
                            true
                        } else {
                            false
                        };

                        if render_with_new_scale_factor {
                            app.resize(window);
                            window.request_redraw();
                        }
                    }
                }

                let name = match state {
                    ElementState::Pressed => EventName::KeyDown,
                    ElementState::Released => EventName::KeyUp,
                };
                self.send_event(PlatformEvent {
                    name,
                    data: PlatformEventData::Keyboard {
                        key: map_winit_key(&logical_key),
                        code: map_winit_physical_key(&physical_key),
                        modifiers: map_winit_modifiers(self.modifiers_state),
                    },
                })
            }
            WindowEvent::CursorLeft { .. } => {
                if self.mouse_state == ElementState::Released {
                    self.cursor_pos = CursorPoint::new(-1.0, -1.0);

                    self.send_event(PlatformEvent {
                        name: EventName::MouseMove,
                        data: PlatformEventData::Mouse {
                            cursor: self.cursor_pos,
                            button: None,
                        },
                    });
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = CursorPoint::from((position.x, position.y));

                self.send_event(PlatformEvent {
                    name: EventName::MouseMove,
                    data: PlatformEventData::Mouse {
                        cursor: self.cursor_pos,
                        button: None,
                    },
                });

                if let Some(dropped_file_path) = self.dropped_file_path.take() {
                    self.send_event(PlatformEvent {
                        name: EventName::FileDrop,
                        data: PlatformEventData::File {
                            file_path: Some(dropped_file_path),
                            cursor: self.cursor_pos,
                        },
                    });
                }
            }
            WindowEvent::Touch(Touch {
                location,
                phase,
                id,
                force,
                ..
            }) => {
                self.cursor_pos = CursorPoint::from((location.x, location.y));

                let name = match phase {
                    TouchPhase::Cancelled => EventName::TouchCancel,
                    TouchPhase::Ended => EventName::TouchEnd,
                    TouchPhase::Moved => EventName::TouchMove,
                    TouchPhase::Started => EventName::TouchStart,
                };

                self.send_event(PlatformEvent {
                    name,
                    data: PlatformEventData::Touch {
                        location: self.cursor_pos,
                        finger_id: id,
                        phase,
                        force,
                    },
                });
            }
            WindowEvent::Resized(size) => {
                let (new_surface, new_dirty_surface) = graphics_driver.resize(size);

                *surface = new_surface;
                *dirty_surface = new_dirty_surface;

                window.request_redraw();

                app.resize(window);
            }
            WindowEvent::DroppedFile(file_path) => {
                self.dropped_file_path = Some(file_path);
            }
            WindowEvent::HoveredFile(file_path) => {
                self.send_event(PlatformEvent {
                    name: EventName::GlobalFileHover,
                    data: PlatformEventData::File {
                        file_path: Some(file_path),
                        cursor: self.cursor_pos,
                    },
                });
            }
            WindowEvent::HoveredFileCancelled => {
                self.send_event(PlatformEvent {
                    name: EventName::GlobalFileHoverCancelled,
                    data: PlatformEventData::File {
                        file_path: None,
                        cursor: self.cursor_pos,
                    },
                });
            }
            WindowEvent::Focused(is_focused) => {
                *is_window_focused = is_focused;
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.run_on_exit();
    }
}
