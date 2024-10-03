use std::{process::Output, time::Duration};

use dioxus_core::prelude::{spawn, use_hook, Task};
use dioxus_hooks::{use_memo, use_reactive, use_signal, Dependency};
use dioxus_signals::{Memo, ReadOnlySignal, Readable, Signal, Writable};
use easer::functions::*;
use freya_engine::prelude::{Color, HSV};
use freya_node_state::Parse;
use tokio::time::Instant;

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

pub struct SegmentCompositor<T: Easable<Output = O> + Clone, O: Clone> {
    segments: Vec<Segment<T, O>>,
    total_duration: u32,
}

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
    fn duration(&self) -> Duration {
        Duration::from_millis(self.total_duration as u64)
    }

    fn value(&self, index: u32) -> Self::Output {
        let mut accumulated_time = 0;
        let mut res = None;
        for segment in &self.segments {
            if index > accumulated_time && index <= accumulated_time + segment.duration {
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

        res.expect("to be filled in")
    }
}

pub trait AnimatedValue {
    type Output;
    fn duration(&self) -> Duration;

    fn value(&self, index: u32) -> Self::Output;
}

pub fn use_animation(run: impl Fn(ctx: ()) -> impl AnimatedValue) -> () {
    
}

// pub type ReadAnimatedValue<O> = ReadOnlySignal<AnimatedValue<Output = O>>;
//
// #[derive(Default, PartialEq, Clone)]
// pub struct Context<A: AnimatedValue<Output = O>, O> {
//     animated_value: Signal<A>,
//     on_finish: OnFinish,
//     auto_start: bool,
// }
//
// impl<A: AnimatedValue<Output = O> + 'static, O> Context<A, O> {
//     pub fn with(&mut self, animated_value: A) -> ReadAnimatedValue<O> {
//         let signal = Signal::new(animated_value);
//         self.animated_value = signal;
//         ReadOnlySignal::new(signal)
//     }
//
//     pub fn on_finish(&mut self, on_finish: OnFinish) -> &mut Self {
//         self.on_finish = on_finish;
//         self
//     }
//
//     pub fn auto_start(&mut self, auto_start: bool) -> &mut Self {
//         self.auto_start = auto_start;
//         self
//     }
// }
//
// /// Controls the direction of the animation.
// #[derive(Clone, Copy)]
// pub enum AnimDirection {
//     Forward,
//     Reverse,
// }
//
// impl AnimDirection {
//     pub fn toggle(&mut self) {
//         match self {
//             Self::Forward => *self = Self::Reverse,
//             Self::Reverse => *self = Self::Forward,
//         }
//     }
// }
//
// /// What to do once the animation finishes. By default it is [`Stop`](OnFinish::Stop)
// #[derive(PartialEq, Clone, Copy, Default)]
// pub enum OnFinish {
//     #[default]
//     Stop,
//     Reverse,
//     Restart,
// }
//
// /// Animate your elements. Use [`use_animation`] to use this.
// #[derive(PartialEq, Clone)]
// pub struct UseAnimator<A: AnimatedValue<Output = O> + 'static, O> {
//     pub(crate) value_and_ctx: Memo<(A, Context<A, O>)>,
//     pub(crate) platform: UsePlatform,
//     pub(crate) is_running: Signal<bool>,
//     pub(crate) has_run_yet: Signal<bool>,
//     pub(crate) task: Signal<Option<Task>>,
//     pub(crate) last_direction: Signal<AnimDirection>,
// }
//
// impl<A: AnimatedValue<Output = O> + 'static, O> Copy for UseAnimator<A, O> {}
//
// impl<A: AnimatedValue<Output = O> + 'static, O> UseAnimator<A, O> {
//     /// Get the animated value.
//     pub fn get(&self) -> A {
//         self.value_and_ctx.read().0.clone()
//     }
//
//     /// Reset the animation to the default state.
//     pub fn reset(&self) {
//         let mut task = self.task;
//
//         if let Some(task) = task.write().take() {
//             task.cancel();
//         }
//
//         let value = self.value_and_ctx.read().1.animated_value;
//         let mut value = *value;
//         value.write().prepare(AnimDirection::Forward);
//     }
//
//     /// Update the animation.
//     pub fn run_update(&self) {
//         let mut task = self.task;
//
//         if let Some(task) = task.write().take() {
//             task.cancel();
//         }
//
//         let value = self.value_and_ctx.read().1.animated_value;
//         let mut value = *value;
//         let time = value.peek().time().as_millis() as i32;
//         value.write().advance(time, *self.last_direction.peek());
//     }
//
//     /// Checks if there is any animation running.
//     pub fn is_running(&self) -> bool {
//         *self.is_running.read()
//     }
//
//     /// Checks if it has run yet, by subscribing.
//     pub fn has_run_yet(&self) -> bool {
//         *self.has_run_yet.read()
//     }
//
//     /// Checks if it has run yet, doesn't subscribe. Useful for when you just mounted your component.
//     pub fn peek_has_run_yet(&self) -> bool {
//         *self.has_run_yet.peek()
//     }
//
//     /// Runs the animation in reverse direction.
//     pub fn reverse(&self) {
//         self.run(AnimDirection::Reverse)
//     }
//
//     /// Runs the animation normally.
//     pub fn start(&self) {
//         self.run(AnimDirection::Forward)
//     }
//
//     /// Run the animation with a given [`AnimDirection`]
//     pub fn run(&self, mut direction: AnimDirection) {
//         let ctx = &self.value_and_ctx.peek().1;
//         let platform = self.platform;
//         let mut is_running = self.is_running;
//         let mut ticker = platform.new_ticker();
//         let mut value = ctx.animated_value;
//         let mut has_run_yet = self.has_run_yet;
//         let on_finish = ctx.on_finish;
//         let mut task = self.task;
//         let mut last_direction = self.last_direction;
//
//         last_direction.set(direction);
//
//         // Cancel previous animations
//         if let Some(task) = task.write().take() {
//             task.cancel();
//         }
//
//         if !self.peek_has_run_yet() {
//             *has_run_yet.write() = true;
//         }
//         is_running.set(true);
//
//         let animation_task = spawn(async move {
//             platform.request_animation_frame();
//
//             let mut index = 0;
//             let mut prev_frame = Instant::now();
//
//             // Prepare the animations with the the proper direction
//             value.write().prepare(direction);
//
//             loop {
//                 // Wait for the event loop to tick
//                 ticker.tick().await;
//                 platform.request_animation_frame();
//
//                 index += prev_frame.elapsed().as_millis() as i32;
//
//                 let is_finished = value.is_finished(index, direction);
//
//                 // Advance the animations
//                 value.write().advance(index, direction);
//
//                 prev_frame = Instant::now();
//
//                 if is_finished {
//                     if OnFinish::Reverse == on_finish {
//                         // Toggle direction
//                         direction.toggle();
//                     }
//                     match on_finish {
//                         OnFinish::Restart | OnFinish::Reverse => {
//                             index = 0;
//
//                             // Restart the animation
//                             value.write().prepare(direction);
//                         }
//                         OnFinish::Stop => {
//                             // Stop if all the animations are finished
//                             break;
//                         }
//                     }
//                 }
//             }
//
//             is_running.set(false);
//             task.write().take();
//         });
//
//         // Cancel previous animations
//         task.write().replace(animation_task);
//     }
// }

// pub fn use_animation<A: AnimatedValue<Output = O> + 'static, O>(
//     run: impl Fn(&mut Context<A, O>) -> A + Clone + 'static,
// ) -> UseAnimator<A, O> {
//     let pltform = use_platform();
//     let is_running = use_signal(|| false);
//     let has_run_yet = use_signal(|| false);
//     let task = use_signal(|| None);
//     let last_direction = use_signal(|| AnimDirection::Reverse);
//
//     let value_and_ctx = use_memo(move || {
//         let mut ctx = Context::default();
//         (run(&mut ctx), ctx)
//     });
//
//     let animator = UseAnimator {
//         value_and_ctx,
//         platform,
//         is_running,
//         has_run_yet,
//         task,
//         last_direction,
//     };
//
//     use_hook(move || {
//         if animator.value_and_ctx.read().1.auto_start {
//             animator.run(AnimDirection::Forward);
//         }
//     });
//
//     animator
// }

// pub fn use_animation_with_dependencies<Animated: PartialEq + Clone + 'static, D: Dependency>(
//     deps: D,
//     run: impl Fn(&mut Context, D::Out) -> Animated + 'static,
// ) -> UseAnimator<Animated>
// where
//     D::Out: 'static + Clone,
// {
//     let platform = use_platform();
//     let is_running = use_signal(|| false);
//     let has_run_yet = use_signal(|| false);
//     let task = use_signal(|| None);
//     let last_direction = use_signal(|| AnimDirection::Reverse);
//
//     let value_and_ctx = use_memo(use_reactive(deps, move |vals| {
//         let mut ctx = Context::default();
//         (run(&mut ctx, vals), ctx)
//     }));
//
//     let animator = UseAnimator {
//         value_and_ctx,
//         platform,
//         is_running,
//         has_run_yet,
//         task,
//         last_direction,
//     };
//
//     use_memo(move || {
//         let _ = value_and_ctx.read();
//         if *has_run_yet.peek() {
//             animator.run_update()
//         }
//     });
//
//     use_hook(move || {
//         if animator.value_and_ctx.read().1.auto_start {
//             animator.run(AnimDirection::Forward);
//         }
//     });
//
//     animator
// }
