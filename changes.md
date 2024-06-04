# 0.20.1

* Extend tasks with cancellation support.
* Add queue for extra result values from event-handling.
  Used for accurate repaint after focus changes.

* fix missing conversion from ScrollOutcome.
* fix missing conversions for DoubleClickOutcome.

* simplified the internal machinery of event-handling a bit.
  Simpler type variables are a thing.

# 0.15.1

was the wrong crate committed

# 0.20.0

* Split AppWidgets into AppWidgets and AppEvents. One for the
  widget side for render, the other for the state side for all
  event handling. This better aligns with the split seen
  in ratatui stateful widgets.
    - The old mono design goes too much in the direction of a widget tree,
      which is not the intent.
    - It seems that AppWidget now very much mimics the StatefulWidget trait,
      event if that was not the initial goal. Curious.
    - I think I'm quite content with the tree-way split that now exists.
    - I had originally intended to use the rat-event::HandleEvent trait
      instead of some AppEvents, but that proved to limited. It still is
      very fine for creating widgets, that's why I don't want to change
      it anymore. Can live well with this current state.

# 0.19.0

First release that I consider as BETA ready.

* reorg from rat-event down. built in some niceties there.

# 0.18.0

Start from scratch as rat-salsa2. The old rat-salsa now is
mostly demolished and split up in

* rat-event
* rat-focus
* rat-ftable
* rat-input
* rat-scrolled
* rat-widget

and the rest is not deemed worth it. 