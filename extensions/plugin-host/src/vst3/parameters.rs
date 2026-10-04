//! The parameters of a VST 3 plugin, as its edit controller describes them.
//!
//! A VST 3 host only ever sees normalized values, from 0 to 1. A parameter with a `stepCount`
//! above zero takes `stepCount + 1` of them, evenly spaced, and `kIsList` says its steps are
//! names to choose from rather than numbers.

use vst3::ComPtr;
use vst3::Steinberg::Vst::{
    IEditController, IEditControllerTrait, ParamID, ParamValue, ParameterInfo,
    ParameterInfo_::ParameterFlags_, String128,
};
use vst3::Steinberg::{int32, kResultOk};

use crate::parameters::{Parameter, Steps};

/// Every parameter a host may set, in the controller's order. Read-only and hidden ones are
/// left out.
///
/// # Safety
///
/// The controller must be alive.
pub(super) unsafe fn of_controller(controller: &ComPtr<IEditController>) -> Vec<Parameter> {
    let has = |info: &ParameterInfo, flag: i32| info.flags & flag != 0;
    let mut parameters = Vec::new();
    // SAFETY: the caller keeps the contract. `info` is written by the plugin before it is
    // read, and a call that fails leaves it untouched, which is why it starts zeroed.
    unsafe {
        for index in 0..controller.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            if controller.getParameterInfo(index, &mut info) != kResultOk {
                continue;
            }
            // A read-only parameter is the plugin's to set, and a hidden one is not for a person.
            let left_out = ParameterFlags_::kIsReadOnly | ParameterFlags_::kIsHidden;
            if has(&info, left_out as int32) {
                continue;
            }
            let steps = (info.stepCount > 0).then(|| {
                let step_count = info.stepCount as u32;
                Steps::new(
                    step_count.saturating_add(1),
                    0.0,
                    1.0,
                    has(&info, ParameterFlags_::kIsList as int32),
                    |value| text(controller, info.id, value),
                )
            });
            parameters.push(Parameter {
                id: info.id,
                name: utf16(&info.title),
                minimum: 0.0,
                maximum: 1.0,
                default: info.defaultNormalizedValue,
                steps,
                automatable: has(&info, ParameterFlags_::kCanAutomate as int32),
            });
        }
    }
    parameters
}

/// The plugin's own text for a normalized value of a parameter.
///
/// # Safety
///
/// The controller must be alive.
pub(super) unsafe fn text(
    controller: &ComPtr<IEditController>,
    id: ParamID,
    value: ParamValue,
) -> Option<String> {
    let mut string: String128 = [0; 128];
    // SAFETY: the caller keeps the contract, and `string` is the 128 units the call writes.
    let result = unsafe { controller.getParamStringByValue(id, value, &mut string) };
    (result == kResultOk).then(|| utf16(&string))
}

/// A fixed UTF-16 buffer of the VST 3 API, up to the zero that ends it. A plugin that filled
/// the whole buffer without one is read to its end, never past it.
fn utf16(buffer: &String128) -> String {
    let text = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16_lossy(text)
}
