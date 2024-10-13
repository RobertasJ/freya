use std::time::Duration;
use std::time::Instant;

use dioxus_core::prelude::{spawn, use_hook, Task};
use dioxus_hooks::{use_effect, use_memo, use_reactive, use_signal, Dependency};
use dioxus_signals::UnsyncStorage;
use dioxus_signals::Write;
use dioxus_signals::{Memo, ReadOnlySignal, Readable, Signal, Writable};
use easer::functions::*;
use freya_engine::prelude::{Color, HSV};
use freya_node_state::Parse;
use std::fmt::Debug;
use torin::direction;
use winit::platform;

use crate::Ticker;
use crate::{use_platform, UsePlatform};
/// ```
/// fn(time: f32, start: f32, end: f32, duration: f32) -> f32;
/// ```
type EasingFunction = fn(f32, f32, f32, f32) -> f32;
pub trait Easable {
    type Output;
    fn ease(self, to: Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output;
}

impl Easable for f32 {
    type Output = Self;
    fn ease(self, to: Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        function(time as f32, self, to - self, duration as f32)
    }
}

impl Easable for Color {
    type Output = Self;
    fn ease(self, to: Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        println!("{self:#?}\n\n{to:#?}");
        let hsv1 = self.to_hsv();
        let hsv2 = to.to_hsv();

        let h = function(time as f32, hsv1.h, hsv2.h - hsv1.h, duration as f32);
        let s = function(time as f32, hsv1.s, hsv2.s - hsv1.s, duration as f32);
        let v = function(time as f32, hsv1.v, hsv2.v - hsv1.v, duration as f32);

        let eased = HSV { h, s, v };
        let color = eased.to_color(255);

        color
    }
}

impl Easable for &str {
    type Output = String;
    fn ease(self, to: Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        let color = Color::parse(self).expect("to be a color").ease(
            Color::parse(to).expect("to be a color"),
            time,
            duration,
            function,
        );
        format!(
            "rgb({}, {}, {}, {})",
            color.r(),
            color.g(),
            color.b(),
            color.a()
        )
    }
}

#[derive(PartialEq, Clone)]
pub struct SegmentCompositor<T: Easable<Output = O> + Clone, O: Clone> {
    segments: Vec<Segment<T, O>>,
    total_duration: u32,
}

#[derive(PartialEq, Clone)]
struct Segment<T: Easable<Output = O> + Clone, O: Clone> {
    start: T,
    end: T,
    duration: u32,
    function: EasingFunction,
}

impl<T: Easable<Output = O> + Clone, O: Clone> SegmentCompositor<T, O> {
    pub fn new(start: T, end: T, duration: u32, function: EasingFunction) -> Self {
        let segment = Segment {
            start: start.clone(),
            end,
            duration,
            function,
        };

        Self {
            total_duration: duration,
            segments: vec![segment],
        }
    }

    pub fn add_segment(
        mut self,
        start: T,
        end: T,
        duration: u32,
        function: EasingFunction,
    ) -> Self {
        let segment = Segment {
            start,
            end,
            duration,
            function,
        };

        self.total_duration += duration;
        self.segments.push(segment);
        self
    }

    pub fn add_constant_segment(mut self, value: T, duration: u32) -> Self {
        let segment = Segment {
            start: value.clone(),
            end: value,
            duration,
            function: |_time: f32, start: f32, _end: f32, _duration: f32| start,
        };

        self.total_duration += duration;
        self.segments.push(segment);
        self
    }
}

impl<T: Easable<Output = O> + Clone, O: Clone> AnimatedValue for SegmentCompositor<T, O> {
    type Output = O;
    fn duration(&self) -> u32 {
        self.total_duration
    }

    fn calc(&self, index: u32) -> Self::Output {
        let mut accumulated_time = 0;
        let mut res = None;
        for segment in &self.segments {
            if index >= accumulated_time && index <= accumulated_time + segment.duration {
                let relative_time = index - accumulated_time;
                res = Some(segment.start.clone().ease(
                    segment.end.clone(),
                    relative_time,
                    segment.duration,
                    segment.function,
                ));
                break;
            }

            accumulated_time += segment.duration;
        }

        res.expect(&format!("to be filled in"))
    }
}

pub trait AnimatedValue {
    type Output;
    fn duration(&self) -> u32;

    fn calc(&self, index: u32) -> Self::Output;
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Context {
    auto_start: bool,
    starting_direction: Direction,
}

impl Context {
    pub fn auto_start(&mut self) -> &mut Self {
        self.auto_start = true;
        self
    }

    pub fn starting_direction(&mut self, direction: Direction) -> &mut Self {
        self.starting_direction = direction;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Clone, Copy)]
pub struct UseAnimator<
    O: 'static + Clone,
    Animated: AnimatedValue<Output = O> + PartialEq + Clone + 'static,
> {
    function_and_ctx: Memo<(Animated, Context)>,
    is_running: Signal<bool>,
    task: Signal<Option<Task>>,
    platform: UsePlatform,
    direction: Signal<Direction>,
    value: Signal<O>,
}

impl<O: 'static + Clone, Animated: AnimatedValue<Output = O> + Clone + PartialEq + 'static>
    UseAnimator<O, Animated>
{
    pub fn run(&mut self, direction: Direction) {
        *self.direction.write() = direction;

        if !(self.is_running)() {
            let direction = self.direction;
            let function_and_ctx = self.function_and_ctx;
            let mut value = self.value;
            let mut is_running = self.is_running;
            let platform = self.platform;
            let mut ticker = platform.new_ticker();

            is_running.set(true);
            let task = spawn(async move {
                platform.request_animation_frame();
                let mut anchor = match *direction.peek() {
                    Direction::Forward => 0,
                    Direction::Backward => {
                        let duration = function_and_ctx.read().0.duration();
                        duration
                    }
                };

                let mut offset = Instant::now();

                let mut last_direction = *direction.peek();

                loop {
                    fn offset_time(
                        direction: Direction,
                        anchor: u32,
                        offset: Instant,
                    ) -> Option<u32> {
                        match direction {
                            Direction::Forward => {
                                let elapsed = offset.elapsed().as_millis() as u32;
                                Some(anchor + elapsed)
                            }
                            Direction::Backward => {
                                let elapsed = offset.elapsed().as_millis() as u32;
                                anchor.checked_sub(elapsed)
                            }
                        }
                    }

                    ticker.tick().await;

                    platform.request_animation_frame();

                    let current_offset_time =
                        offset_time(last_direction, anchor, offset).unwrap_or(0);

                    if current_offset_time == 0 {
                        *value.write() = function_and_ctx.read().0.calc(0);

                        *is_running.write() = false;
                        break;
                    }

                    if current_offset_time >= function_and_ctx.read().0.duration() {
                        *value.write() = function_and_ctx
                            .read()
                            .0
                            .calc(function_and_ctx.read().0.duration());

                        *is_running.write() = false;
                        break;
                    }

                    if !is_running() {}

                    if last_direction != *direction.peek() {
                        println!("direction changed");
                        anchor =
                            offset_time(last_direction, anchor, offset).expect("to not underflow");
                        offset = Instant::now();
                        last_direction = *direction.peek();
                    }

                    *value.write() = function_and_ctx.read().0.calc(
                        offset_time(*direction.peek(), anchor, offset).expect("to not underflow"),
                    );
                }
            });

            let mut x: Write<Option<Task>, UnsyncStorage> = self.task.write();
            x.replace(task);

            return;
        }
    }

    pub fn value(&self) -> ReadOnlySignal<O> {
        ReadOnlySignal::new(self.value)
    }
}

pub fn use_animation<
    O: 'static + Clone,
    Animated: AnimatedValue<Output = O> + Clone + PartialEq + 'static,
>(
    run: impl Fn(&mut Context) -> Animated + 'static,
) -> UseAnimator<O, Animated> {
    let function_and_ctx = use_memo(move || {
        let mut ctx = Context {
            auto_start: false,
            starting_direction: Direction::Forward,
        };
        (run(&mut ctx), ctx)
    });

    let task = use_signal(|| None);
    let platform = use_platform();
    let is_running = use_signal(move || false);
    let direction = use_signal(move || function_and_ctx.read().1.starting_direction);
    let value = use_signal(move || {
        let time = match *direction.peek() {
            Direction::Forward => 0,
            Direction::Backward => function_and_ctx.read().0.duration(),
        };
        function_and_ctx.read().0.calc(time)
    });

    use_hook(move || {
        let mut animator = UseAnimator {
            function_and_ctx,
            is_running,
            direction,
            platform,
            task,
            value,
        };
        let ctx = animator.function_and_ctx.read().1;

        if ctx.auto_start {
            animator.run(ctx.starting_direction);
        }

        animator
    })
}
