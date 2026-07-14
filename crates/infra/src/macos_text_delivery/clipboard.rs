use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardContentsOptions, NSPasteboardItem, NSPasteboardTypeString,
    NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString};
use template_app::TextDeliveryError;

pub(super) const TRANSIENT_PASTEBOARD_TYPE: &str = "org.nspasteboard.TransientType";

pub(super) fn copy_text(text: &str) -> Result<(), TextDeliveryError> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let text = NSString::from_str(text);
    let text: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
        ProtocolObject::from_retained(text);
    let objects = NSArray::from_retained_slice(&[text]);
    pasteboard.clearContents();
    if pasteboard.writeObjects(&objects) {
        Ok(())
    } else {
        Err(pasteboard_error("copy transcript"))
    }
}

pub(super) struct TemporaryPasteboard {
    pasteboard: Retained<NSPasteboard>,
    snapshot: PasteboardSnapshot,
    temporary_change_count: isize,
}

impl TemporaryPasteboard {
    pub(super) fn replace(
        pasteboard: Retained<NSPasteboard>,
        text: &str,
    ) -> Result<Self, TextDeliveryError> {
        let snapshot = PasteboardSnapshot::capture(&pasteboard)?;
        let item = temporary_item(text)?;
        let item: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
            ProtocolObject::from_retained(item);
        let objects = NSArray::from_retained_slice(&[item]);

        let prepared_change_count = pasteboard
            .prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
        if !pasteboard.writeObjects(&objects) {
            restore_snapshot_if_unchanged(snapshot, &pasteboard, prepared_change_count)?;
            return Err(pasteboard_error("write temporary transcript"));
        }

        let temporary_change_count = pasteboard.changeCount();
        Ok(Self {
            pasteboard,
            snapshot,
            temporary_change_count,
        })
    }

    pub(super) fn general(text: &str) -> Result<Self, TextDeliveryError> {
        Self::replace(NSPasteboard::generalPasteboard(), text)
    }

    pub(super) fn restore_if_unchanged(self) -> Result<(), TextDeliveryError> {
        restore_snapshot_if_unchanged(self.snapshot, &self.pasteboard, self.temporary_change_count)
    }
}

fn restore_snapshot_if_unchanged(
    snapshot: PasteboardSnapshot,
    pasteboard: &NSPasteboard,
    temporary_change_count: isize,
) -> Result<(), TextDeliveryError> {
    if pasteboard.changeCount() == temporary_change_count {
        snapshot.restore(pasteboard)
    } else {
        Ok(())
    }
}

fn temporary_item(text: &str) -> Result<Retained<NSPasteboardItem>, TextDeliveryError> {
    let item = NSPasteboardItem::new();
    let text = NSString::from_str(text);
    let transient_type = NSString::from_str(TRANSIENT_PASTEBOARD_TYPE);
    let transient_value = NSString::new();
    // SAFETY: AppKit initializes this immutable pasteboard type constant before use.
    let string_type = unsafe { NSPasteboardTypeString };
    if item.setString_forType(&text, string_type)
        && item.setString_forType(&transient_value, &transient_type)
    {
        Ok(item)
    } else {
        Err(pasteboard_error("build temporary transcript item"))
    }
}

struct PasteboardSnapshot(Vec<Vec<(Retained<NSString>, Retained<NSData>)>>);

impl PasteboardSnapshot {
    fn capture(pasteboard: &NSPasteboard) -> Result<Self, TextDeliveryError> {
        let items = pasteboard
            .pasteboardItems()
            .map(|items| items.to_vec())
            .unwrap_or_default();
        let snapshot = items
            .iter()
            .map(|item| {
                capture_all_fields(item.types().to_vec(), |pasteboard_type| {
                    item.dataForType(pasteboard_type)
                })
                .ok_or_else(|| pasteboard_error("capture every clipboard type"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self(snapshot))
    }

    fn restore(self, pasteboard: &NSPasteboard) -> Result<(), TextDeliveryError> {
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
            .0
            .into_iter()
            .map(|fields| {
                let item = NSPasteboardItem::new();
                for (pasteboard_type, data) in fields {
                    if !item.setData_forType(&data, &pasteboard_type) {
                        return Err(pasteboard_error("restore every clipboard type"));
                    }
                }
                Ok(ProtocolObject::from_retained(item))
            })
            .collect::<Result<Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>>, _>>()?;
        let objects = NSArray::from_retained_slice(&items);
        pasteboard.clearContents();
        if items.is_empty() || pasteboard.writeObjects(&objects) {
            Ok(())
        } else {
            Err(pasteboard_error("restore clipboard"))
        }
    }
}

fn capture_all_fields<T, U>(
    fields: impl IntoIterator<Item = T>,
    mut read: impl FnMut(&T) -> Option<U>,
) -> Option<Vec<(T, U)>> {
    fields
        .into_iter()
        .map(|field| read(&field).map(|value| (field, value)))
        .collect()
}

fn pasteboard_error(operation: &str) -> TextDeliveryError {
    TextDeliveryError::System(format!("failed to {operation}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_item_marks_text_transient() {
        let Ok(item) = temporary_item("dictated text") else {
            panic!("temporary pasteboard item should be writable");
        };
        // SAFETY: AppKit initializes this immutable pasteboard type constant before use.
        let string_type = unsafe { NSPasteboardTypeString };
        assert_eq!(
            Some("dictated text".to_owned()),
            item.stringForType(string_type)
                .map(|value| value.to_string())
        );
        let transient_type = NSString::from_str(TRANSIENT_PASTEBOARD_TYPE);
        assert_eq!(
            Some(String::new()),
            item.stringForType(&transient_type)
                .map(|value| value.to_string())
        );
    }

    #[test]
    fn snapshot_requires_data_for_every_declared_type() {
        assert_eq!(
            Some(vec![(1, "one"), (2, "two")]),
            capture_all_fields([1, 2], |field| match field {
                1 => Some("one"),
                2 => Some("two"),
                _ => None,
            })
        );
        assert_eq!(
            None,
            capture_all_fields([1, 2], |field| (*field == 1).then_some("one"))
        );
    }
}
