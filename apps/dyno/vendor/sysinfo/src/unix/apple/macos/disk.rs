// Take a look at the license at the top of the repository in the LICENSE file.

use crate::DiskKind;
use crate::sys::{
    disk::{get_int_value, get_str_value},
    macos::{
        ffi,
        utils::{IOReleaser, MAIN_PORT},
    },
};

use objc2_core_foundation::{CFDictionary, CFRetained, CFString, kCFAllocatorDefault};
use objc2_io_kit::{
    IOBSDNameMatching, IOIteratorNext, IOObjectConformsTo, IORegistryEntryCreateCFProperty,
    IORegistryEntryGetParentEntry, IOServiceGetMatchingServices, io_iterator_t,
    io_registry_entry_t, kIOServicePlane,
};

/// The iterator of an `IOServiceGetMatchingServices` call, or `None` when the call failed or
/// found nothing.
fn matching_services(result: libc::kern_return_t, iterator: io_iterator_t) -> Option<IOReleaser> {
    if result != libc::KERN_SUCCESS {
        return None;
    }
    // IOServiceGetMatchingServices can return success with a null iterator on some macOS
    // setups. Never construct a NonZero io_object_t from 0 (debug panic / release UB).
    match IOReleaser::new(iterator) {
        Some(iterator) => Some(iterator),
        None => {
            sysinfo_debug!("IOServiceGetMatchingServices succeeded but returned a null iterator");
            None
        }
    }
}

fn iterate_service_tree<T, F>(bsd_name: &[u8], key: &CFString, eval: F) -> Option<T>
where
    F: Fn(io_registry_entry_t, &CFDictionary) -> Option<T>,
{
    let matching = unsafe { IOBSDNameMatching(*MAIN_PORT, 0, bsd_name.as_ptr().cast()) }?;
    let matching = CFRetained::<CFDictionary>::from(&matching);

    let mut service_iterator: io_iterator_t = 0;

    let result =
        unsafe { IOServiceGetMatchingServices(*MAIN_PORT, Some(matching), &mut service_iterator) };
    let service_iterator = matching_services(result, service_iterator)?;

    let mut parent_entry: io_registry_entry_t = 0;

    while let Some(mut current_service_entry) =
        IOReleaser::new(IOIteratorNext(service_iterator.inner()))
    {
        // Note: This loop is required in a non-obvious way. Due to device properties existing as a tree
        // in IOKit, we may need an arbitrary number of calls to `IORegistryEntryCreateCFProperty` in order to find
        // the values we are looking for. The function may return nothing if we aren't deep enough into the registry
        // tree, so we need to continue going from child->parent node until its found.
        loop {
            if unsafe {
                IORegistryEntryGetParentEntry(
                    current_service_entry.inner(),
                    kIOServicePlane.as_ptr().cast_mut().cast(),
                    &mut parent_entry,
                )
            } != libc::KERN_SUCCESS
            {
                break;
            }

            current_service_entry = match IOReleaser::new(parent_entry) {
                Some(service) => service,
                // There were no more parents left
                None => break,
            };

            let properties_result = unsafe {
                IORegistryEntryCreateCFProperty(
                    current_service_entry.inner(),
                    Some(key),
                    kCFAllocatorDefault,
                    0,
                )
            };

            if let Some(properties) = properties_result
                && let Ok(properties) = properties.downcast::<CFDictionary>()
                && let Some(result) = eval(parent_entry, &properties)
            {
                return Some(result);
            }
        }
    }

    None
}

pub(crate) fn get_disk_type(bsd_name: &[u8]) -> Option<DiskKind> {
    let characteristics_string =
        CFString::from_static_str(ffi::kIOPropertyDeviceCharacteristicsKey);

    iterate_service_tree(bsd_name, &characteristics_string, |_, properties| {
        let medium = unsafe {
            super::disk::get_str_value(
                properties,
                Some(&CFString::from_static_str(ffi::kIOPropertyMediumTypeKey)),
            )
        }?;

        match medium.as_str() {
            _ if medium == ffi::kIOPropertyMediumTypeSolidStateKey => Some(DiskKind::SSD),
            _ if medium == ffi::kIOPropertyMediumTypeRotationalKey => Some(DiskKind::HDD),
            _ => Some(DiskKind::Unknown(-1)),
        }
    })
}

/// Returns a tuple consisting of the total number of bytes read and written by the specified disk
pub(crate) fn get_disk_io(bsd_name: &[u8]) -> Option<(u64, u64)> {
    let stat_string = CFString::from_static_str(ffi::kIOBlockStorageDriverStatisticsKey);

    iterate_service_tree(bsd_name, &stat_string, |parent_entry, properties| {
        if !unsafe { IOObjectConformsTo(parent_entry, c"IOBlockStorageDriver".as_ptr() as *mut _) }
        {
            return None;
        }

        unsafe {
            let read_bytes = super::disk::get_int_value(
                properties,
                Some(&CFString::from_static_str(
                    ffi::kIOBlockStorageDriverStatisticsBytesReadKey,
                )),
            )?;
            let written_bytes = super::disk::get_int_value(
                properties,
                Some(&CFString::from_static_str(
                    ffi::kIOBlockStorageDriverStatisticsBytesWrittenKey,
                )),
            )?;

            Some((read_bytes, written_bytes))
        }
    })
}

// Paddock (apps/dyno/vendor/PATCHES.md): IOKit can answer KERN_SUCCESS with a null iterator (IOKitLib.h: "If NULL is
// returned, the iteration was successful but found no matching services"), for example for a disk detached between the
// mount listing and this lookup.
#[cfg(test)]
mod paddock_tests {
    #[test]
    fn a_successful_call_with_a_null_iterator_gives_nothing_and_never_panics() {
        assert!(super::matching_services(libc::KERN_SUCCESS, 0).is_none());
    }

    #[test]
    fn a_failed_call_gives_nothing() {
        assert!(super::matching_services(libc::KERN_FAILURE, 0).is_none());
    }

    // Through IOKit: on macOS 27.2 a BSD name with no service gets a null iterator; other versions may answer with an
    // empty one, which this test then cannot tell apart from the fix (the two tests above can).
    #[test]
    fn a_bsd_name_with_no_io_service_gives_nothing_and_never_panics() {
        let gone = b"paddock-no-such-disk\0";
        assert_eq!(super::get_disk_type(gone), None);
        assert_eq!(super::get_disk_io(gone), None);
    }
}
