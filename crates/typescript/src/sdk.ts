// The SDK of the project's own tools. Sound Tools writes this file and the folder `sdk/` when
// the project opens, so an edit there is lost. Read `agent-docs/extensions.md` first.
//
// A tool is a `.ts` or `.tsx` file next to this one that calls `tool` and imports from `./sdk`.
// Below is every name it may import, by the module of `sdk/` that declares it with its doc.

// the fields of a record and the inputs of a tool
export {
  bang, boolean, choice, file, list, noteFields, notes, number, pattern, text
} from "./sdk/fields";
export type {
  BangInput, BooleanField, ChoiceField, Field, Fields, FileField, Inputs, ListField, Note,
  NumberField, ObjectItem, SoundOf, StateOf, TextField, Unit, ValuesOf
} from "./sdk/fields";
// signals: what a sound is made of
export {
  abs, adsr, allpass, bandpass, beat, bpm, Buffer, buffer, change, channel, clamp, cos, db, delay,
  exp, Feedback, feedback, floor, graphToJson, highpass, highshelf, hold, input, inputLeft,
  inputRight, log, lookup, lowpass, lowshelf, max, min, mix, mtof, noise, notch, note, output,
  Param, peak, phasor, PI, playing, pow, rise, sampleRate, saturate, saw, Signal, sin, sine,
  smooth, spectral, sqrt, square, Table, tan, tanh, TAU, triangle, wrap
} from "./sdk/sound";
export type {
  Bin, Bins, Graph, GraphNode, Operand, Stereo, TableRef
} from "./sdk/sound";
// `tool`, its `Context`, children and connections
export {
  takes, tool
} from "./sdk/tools";
export type {
  At, ChildBang, ChildNotes, ChildNumber, ChildPort, ChildTools, Connection, Context, Declaration,
  DraftChildren, FilledStateOfTool, Frozen, HeardEvent, Key, NewChild, NoteEvent, OwnAudio,
  OwnedChild, OwnNotes, Parent, Part, Performer, ProjectTool, SendTo, StateOfTool, Take, Takes,
  TakingTool, Tick, ToolName, ToolSpec
} from "./sdk/tools";
// live values (`now`), drags, pointer events, keys and menus
export {
  Live, now
} from "./sdk/live";
export type {
  Channel, Clock, Drag, DragResult, DragTurns, DragValues, Dropped, KeyName, MenuItem,
  MenuOptions, Modifiers, MovedNumber, Now, Num, Offer, Offers, PlainKey, PointerEvents, Press,
  Section, SpaceValues, TakeList, TempoMap, View, WaveList
} from "./sdk/live";
// what runs again when a value changes: `memo`, `signal`, `timeout`
export {
  batch, interval, memo, onCleanup, selector, signal, timeout, untrack
} from "./sdk/reactivity";
export type {
  Accessor, Bind, Memory
} from "./sdk/reactivity";
// `For`, `Show`, `Switch`, `Popover` and `Dialog`
export {
  Dialog, For, Fragment, Match, Popover, Show, Switch
} from "./sdk/nodes";
export type {
  Child, OverlayProps, ShapeLiteral, UiNode
} from "./sdk/nodes";
// the elements of a view: `div` and its `Style`, `Space` and its shapes, `Button`, `Input`
export {
  Arc, blend, Button, Circle, DeviceCard, Group, h, Icon, Input, Label, Line, Notes, Path,
  PeakMeter, Rect, Space, TitleBar, Toggle, Wave
} from "./sdk/drawing";
export type {
  ButtonProps, CardSelect, Color, DeviceCardProps, IconName, InputProps, Look, Paint, PathPoint,
  Shape, SpaceProps, Style, TextPart, ThemeColor, ToggleProps
} from "./sdk/drawing";
// controls made of them: `Knob`, `Fader`, `Select`, `Steps`, `Pad`
export {
  defaultView, Fader, Knob, Meter, Notice, NumberBox, Pad, positionOf, SegmentedControl, Select,
  Slider, Steps, valueAt, words
} from "./sdk/components";
export type {
  FaderProps, Kept, KnobProps, NumberBoxProps, PadProps, Peaks, SegmentedControlProps,
  SelectOption, SelectProps, SliderProps, StepsProps, Travel, ValueProps
} from "./sdk/components";
// what the runtime calls
export {
  host
} from "./sdk/host";
// the records of the built-in tools, from their Rust types
export * from "./sdk/records";
