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
    fn ease(&self, to: &Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output;
}

impl Easable for f32 {
    type Output = Self;
    fn ease(&self, to: &Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        function(time as f32, *self, *to - *self, duration as f32)
    }
}

impl Easable for Color {
    type Output = Self;
    fn ease(&self, to: &Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        let hsv1 = self.to_hsv();
        let hsv2 = to.to_hsv();

        let h = function(time as f32, hsv1.h, hsv2.h - hsv1.h, duration as f32);
        let s = function(time as f32, hsv1.s, hsv2.s - hsv1.s, duration as f32);
        let v = function(time as f32, hsv1.v, hsv2.v - hsv1.v, duration as f32);

        let eased = HSV { h, s, v };
        let color = eased.to_color(1);

        color
    }
}

impl Easable for &str {
    type Output = String;
    fn ease(&self, to: &Self, time: u32, duration: u32, function: EasingFunction) -> Self::Output {
        let color = &(Color::parse(self)).expect("to be a color").ease(
            &Color::parse(to).unwrap(),
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
        println!("{index}");
        let mut accumulated_time = 0;
        let mut res = None;
        for segment in &self.segments {
            if index >= accumulated_time && index <= accumulated_time + segment.duration {
                let relative_time = index - accumulated_time;
                res = Some(segment.start.ease(
                    &segment.end,
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

#[derive(PartialEq, Eq, Clone)]
pub struct Context {
    auto_start: bool,
}

impl Context {
    pub fn auto_start(&mut self, auto_start: bool) -> &mut Self {
        self.auto_start = auto_start;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Forward,
    Backward,
}

pub struct UseAnimator<O: 'static, Animated: AnimatedValue<Output = O> + PartialEq + 'static> {
    function_and_ctx: Memo<(Animated, Context)>,
    is_running: Signal<bool>,
    task: Signal<Option<Task>>,
    platform: UsePlatform,
    direction: Signal<Direction>,
    value: Signal<O>,
}

impl<O: 'static + Debug, Animated: AnimatedValue<Output = O> + PartialEq + 'static>
    UseAnimator<O, Animated>
{
    pub fn run(&mut self, direction: Direction) {
        *self.direction.write() = direction;

        if !(self.is_running)() {
            println!("Starting the run function...");

            let direction = self.direction;
            let function_and_ctx = self.function_and_ctx;
            let mut value = self.value;
            let mut is_running = self.is_running;
            let platform = self.platform;
            let mut ticker = platform.new_ticker();

            println!("Initial direction: {:?}", direction);
            println!("Initial value: {:?}", value.read());
            println!("Is running initially: {:?}", is_running());

            let task = spawn(async move {
                let mut anchor = match *direction.peek() {
                    Direction::Forward => {
                        println!("Direction is Forward, setting anchor to 0");
                        0
                    }
                    Direction::Backward => {
                        let duration = function_and_ctx.read().0.duration();
                        println!(
                            "Direction is Backward, setting anchor to duration: {:?}",
                            duration
                        );
                        duration
                    }
                };

                let mut offset = Instant::now();
                println!("Offset time initialized: {:?}", offset);

                let mut last_direction = *direction.peek();
                println!("Last direction set to: {:?}", last_direction);

                loop {
                    fn offset_time(
                        direction: Direction,
                        anchor: u32,
                        offset: Instant,
                    ) -> Option<u32> {
                        match direction {
                            Direction::Forward => {
                                let elapsed = offset.elapsed().as_millis() as u32;
                                println!("Direction is Forward, calculating new offset time: anchor + elapsed = {} + {}", anchor, elapsed);
                                Some(anchor + elapsed)
                            }
                            Direction::Backward => {
                                let elapsed = offset.elapsed().as_millis() as u32;
                                println!("Direction is Backward, calculating new offset time: anchor - elapsed = {} - {}", anchor, elapsed);
                                anchor.checked_sub(elapsed)
                            }
                        }
                    }

                    ticker.tick().await;
                    println!("Ticker ticked...");

                    platform.request_animation_frame();
                    println!("Requested animation frame");

                    let current_offset_time =
                        offset_time(*direction.peek(), anchor, offset).unwrap_or(0);
                    println!("Current offset time: {:?}", current_offset_time);

                    if current_offset_time == 0 {
                        println!("Offset time is zero, calculating final value...");
                        *value.write() = function_and_ctx
                            .read()
                            .0
                            .calc(function_and_ctx.read().0.duration());

                        println!("Final value set to: {:?}", *value.write());
                        *is_running.write() = false;
                        println!("Stopping as value is zero...");
                    }

                    if current_offset_time >= function_and_ctx.read().0.duration() {
                        println!("Offset time >= duration, stopping...");
                        *value.write() = function_and_ctx
                            .read()
                            .0
                            .calc(function_and_ctx.read().0.duration());

                        println!("Final value set to: {:?}", *value.write());
                        *is_running.write() = false;
                        println!("Setting is_running to false and exiting...");
                    }

                    if !is_running() {
                        println!("Not running anymore, breaking out of loop...");
                        break;
                    }

                    if last_direction != *direction.peek() {
                        println!(
                            "Direction has changed! Last direction: {:?}, New direction: {:?}",
                            last_direction,
                            *direction.peek()
                        );
                        anchor =
                            offset_time(last_direction, anchor, offset).expect("to not underflow");
                        println!("New anchor calculated after direction change: {:?}", anchor);
                        offset = Instant::now();
                        println!("Resetting offset to now: {:?}", offset);
                    }

                    *value.write() = function_and_ctx.read().0.calc(
                        offset_time(*direction.peek(), anchor, offset).expect("to not underflow"),
                    );
                    println!("Updated value: {:?}", *value.write());
                }
            });

            println!("Spawning task...");
            let mut x: Write<Option<Task>, UnsyncStorage> = self.task.write();
            x.replace(task);

            println!("Task replaced, returning from run function.");
            return;
        } else {
            println!("Already running, exiting early.");
        }
    }

    pub fn value(&self) -> Signal<O> {
        self.value
    }
}

pub fn use_animation<O: 'static, Animated: AnimatedValue<Output = O> + PartialEq + 'static>(
    run: impl Fn(&mut Context) -> Animated + 'static,
) -> UseAnimator<O, Animated> {
    let function_and_ctx = use_memo(move || {
        let mut ctx = Context { auto_start: false };
        (run(&mut ctx), ctx)
    });

    let task = use_signal(|| None);
    let platform = use_platform();
    let is_running = use_signal(move || function_and_ctx.read().1.auto_start);
    let direction = use_signal(move || Direction::Forward);
    let value = use_signal(move || {
        let time = match *direction.peek() {
            Direction::Forward => 0,
            Direction::Backward => function_and_ctx.read().0.duration(),
        };
        function_and_ctx.read().0.calc(time)
    });

    let mut animator = UseAnimator {
        function_and_ctx,
        is_running,
        direction,
        platform,
        task,
        value,
    };

    animator
}
