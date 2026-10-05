use std::{
    any::Any,
    boxed::Box,
    ffi::CString,
    sync::{Arc, Mutex, MutexGuard},
};

use super::{
    get_service_type_support_handle, get_type_support_library, DynamicMessage,
    DynamicMessageMetadata, MessageTypeName, ServiceTypeName,
    INTROSPECTION_TYPE_SUPPORT_IDENTIFIER, REGULAR_TYPE_SUPPORT_IDENTIFIER,
};
use crate::{
    rcl_bindings::*, NodeHandle, RclPrimitive, RclPrimitiveHandle, RclPrimitiveKind, RclReturnCode,
    RclrsError, ReadyKind, ServiceOptions, ToResult, Waitable, WaitableLifecycle, WorkerCommands,
    ENTITY_LIFECYCLE_MUTEX,
};

/// The callback type for a [`GenericService`].
///
/// It receives the request message and a freshly-created (empty) response message, and must
/// return the response message to send. The callback runs synchronously on the executor spin
/// thread, so it should not block for long periods.
pub type GenericServiceCallback =
    Box<dyn FnMut(DynamicMessage, DynamicMessage) -> DynamicMessage + Send>;

/// A service server whose request/response types are only known at runtime.
///
/// Create one using [`NodeState::create_generic_service`][1].
///
/// [1]: crate::NodeState::create_generic_service
pub type GenericService = Arc<GenericServiceState>;

/// The inner state of a [`GenericService`].
pub struct GenericServiceState {
    handle: Arc<GenericServiceHandle>,
    #[allow(unused)]
    callback: Arc<Mutex<GenericServiceCallback>>,
    #[allow(unused)]
    lifecycle: WaitableLifecycle,
    #[allow(dead_code)]
    request_metadata: Arc<DynamicMessageMetadata>,
    #[allow(dead_code)]
    response_metadata: Arc<DynamicMessageMetadata>,
    // This is the regular (non-introspection) type support library that backs the rcl service.
    #[allow(dead_code)]
    type_support_library: Arc<libloading::Library>,
}

impl GenericServiceState {
    /// Returns the name of the service after remapping.
    pub fn service_name(&self) -> String {
        self.handle.service_name()
    }

    /// Creates a new generic service.
    pub(crate) fn create<'a>(
        service_type: ServiceTypeName,
        options: impl Into<ServiceOptions<'a>>,
        callback: GenericServiceCallback,
        node_handle: &Arc<NodeHandle>,
        commands: &Arc<WorkerCommands>,
    ) -> Result<Arc<Self>, RclrsError> {
        let ServiceOptions { name, qos } = options.into();

        // Build the request/response metadata from the introspection service type support.
        let introspection_library = get_type_support_library(
            &service_type.package_name,
            INTROSPECTION_TYPE_SUPPORT_IDENTIFIER,
        )?;
        // SAFETY: The symbol type is trusted assuming the install dir hasn't been tampered with.
        // The pointer is kept valid by keeping the library loaded.
        let service_type_support_ptr = unsafe {
            get_service_type_support_handle(
                introspection_library.as_ref(),
                INTROSPECTION_TYPE_SUPPORT_IDENTIFIER,
                &service_type,
            )?
        };
        // SAFETY: The pointer returned above is valid while the library is loaded.
        let service_type_support = unsafe { &*service_type_support_ptr };
        let request_members =
            unsafe { (*service_type_support.request_typesupport).data as *const _ };
        let response_members =
            unsafe { (*service_type_support.response_typesupport).data as *const _ };
        let request_type = MessageTypeName {
            package_name: service_type.package_name.clone(),
            type_name: format!("{}_Request", service_type.type_name),
        };
        let response_type = MessageTypeName {
            package_name: service_type.package_name.clone(),
            type_name: format!("{}_Response", service_type.type_name),
        };
        // SAFETY: The members pointers come from the introspection library, which we retain.
        let request_metadata = unsafe {
            DynamicMessageMetadata::from_message_members(
                Arc::clone(&introspection_library),
                request_members,
                request_type,
            )
        };
        let response_metadata = unsafe {
            DynamicMessageMetadata::from_message_members(
                Arc::clone(&introspection_library),
                response_members,
                response_type,
            )
        };

        // Load the regular type support handle used to initialize the rcl service.
        let type_support_library =
            get_type_support_library(&service_type.package_name, REGULAR_TYPE_SUPPORT_IDENTIFIER)?;
        // SAFETY: The symbol type is trusted assuming the install dir hasn't been tampered with.
        let type_support = unsafe {
            get_service_type_support_handle(
                type_support_library.as_ref(),
                REGULAR_TYPE_SUPPORT_IDENTIFIER,
                &service_type,
            )?
        };

        let name_c_string = CString::new(name).map_err(|err| RclrsError::StringContainsNul {
            err,
            s: name.into(),
        })?;

        // SAFETY: No preconditions for this function.
        let mut service_options = unsafe { rcl_service_get_default_options() };
        service_options.qos = qos.into();
        // SAFETY: Getting a zero-initialized value is always safe.
        let mut rcl_service = unsafe { rcl_get_zero_initialized_service() };
        {
            let rcl_node = node_handle.rcl_node.lock().unwrap();
            let _lifecycle_lock = ENTITY_LIFECYCLE_MUTEX.lock().unwrap();
            unsafe {
                // SAFETY:
                // * The rcl_service is zero-initialized as mandated by this function.
                // * The rcl_node is kept alive by the NodeHandle.
                // * The name and options are copied by this function.
                // * The entity lifecycle mutex is locked to protect rmw global state.
                rcl_service_init(
                    &mut rcl_service,
                    &*rcl_node,
                    type_support,
                    name_c_string.as_ptr(),
                    &service_options as *const _,
                )
                .ok()?;
            }
        }

        let handle = Arc::new(GenericServiceHandle {
            rcl_service: Mutex::new(rcl_service),
            node_handle: Arc::clone(node_handle),
        });

        let callback = Arc::new(Mutex::new(callback));
        let request_metadata = Arc::new(request_metadata);
        let response_metadata = Arc::new(response_metadata);

        let (waitable, lifecycle) = Waitable::new(
            Box::new(GenericServiceExecutable {
                handle: Arc::clone(&handle),
                callback: Arc::clone(&callback),
                request_metadata: Arc::clone(&request_metadata),
                response_metadata: Arc::clone(&response_metadata),
            }),
            Some(Arc::clone(commands.get_guard_condition())),
        );
        commands.add_to_wait_set(waitable);

        Ok(Arc::new(Self {
            handle,
            callback,
            lifecycle,
            request_metadata,
            response_metadata,
            type_support_library,
        }))
    }
}

struct GenericServiceExecutable {
    handle: Arc<GenericServiceHandle>,
    callback: Arc<Mutex<GenericServiceCallback>>,
    request_metadata: Arc<DynamicMessageMetadata>,
    response_metadata: Arc<DynamicMessageMetadata>,
}

impl RclPrimitive for GenericServiceExecutable {
    unsafe fn execute(
        &mut self,
        ready: ReadyKind,
        _payload: &mut dyn Any,
    ) -> Result<(), RclrsError> {
        ready.for_basic()?;

        let mut request = self.request_metadata.create()?;
        // SAFETY: A zeroed rmw_request_id_t is a valid value.
        let mut request_id = std::mem::zeroed::<rmw_request_id_t>();
        let request_ptr = request.storage.as_mut_ptr();
        let take_result = {
            // SAFETY: All three pointers are valid and initialized.
            rcl_take_request(&*self.handle.lock(), &mut request_id, request_ptr as *mut _).ok()
        };
        match take_result {
            Ok(()) => {}
            Err(RclrsError::RclError {
                code: RclReturnCode::ServiceTakeFailed,
                ..
            }) => {
                // Spurious wakeup, nothing to do.
                return Ok(());
            }
            Err(err) => return Err(err),
        }

        let response_proto = self.response_metadata.create()?;
        let mut response = (self.callback.lock().unwrap())(request, response_proto);
        let response_ptr = response.storage.as_mut_ptr();
        // SAFETY: The response was created from the matching response metadata.
        rcl_send_response(
            &*self.handle.lock(),
            &mut request_id,
            response_ptr as *mut _,
        )
        .ok()
    }

    fn kind(&self) -> RclPrimitiveKind {
        RclPrimitiveKind::Service
    }

    fn handle(&self) -> RclPrimitiveHandle<'_> {
        RclPrimitiveHandle::Service(self.handle.lock())
    }
}

/// Manage the lifecycle of an `rcl_service_t` for a generic service.
struct GenericServiceHandle {
    rcl_service: Mutex<rcl_service_t>,
    node_handle: Arc<NodeHandle>,
}

impl GenericServiceHandle {
    fn lock(&self) -> MutexGuard<'_, rcl_service_t> {
        self.rcl_service.lock().unwrap()
    }

    fn service_name(&self) -> String {
        // SAFETY: The service handle is valid because its lifecycle is managed by an Arc.
        unsafe {
            let raw_service_pointer = rcl_service_get_service_name(&*self.lock());
            std::ffi::CStr::from_ptr(raw_service_pointer)
        }
        .to_string_lossy()
        .into_owned()
    }
}

impl Drop for GenericServiceHandle {
    fn drop(&mut self) {
        let rcl_service = self.rcl_service.get_mut().unwrap();
        let mut rcl_node = self.node_handle.rcl_node.lock().unwrap();
        let _lifecycle_lock = ENTITY_LIFECYCLE_MUTEX.lock().unwrap();
        // SAFETY: The entity lifecycle mutex is locked to protect rmw global state.
        unsafe {
            rcl_service_fini(rcl_service, &mut *rcl_node);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    fn set_int64(message: &mut DynamicMessage, field: &str, value: i64) {
        let crate::dynamic_message::ValueMut::Simple(
            crate::dynamic_message::SimpleValueMut::Int64(v),
        ) = message.get_mut(field).unwrap()
        else {
            panic!("field {} was not an int64", field);
        };
        *v = value;
    }

    fn get_int64(message: &DynamicMessage, field: &str) -> i64 {
        let crate::dynamic_message::Value::Simple(crate::dynamic_message::SimpleValue::Int64(v)) =
            message.get(field).unwrap()
        else {
            panic!("field {} was not an int64", field);
        };
        *v
    }

    #[test]
    fn test_serialized_service_round_trip() -> Result<(), RclrsError> {
        let mut executor = Context::default().create_basic_executor();
        let node = executor
            .create_node(format!("test_serialized_service_{}", line!()).as_str())
            .unwrap();

        let client = node.create_generic_client(
            "example_interfaces/srv/AddTwoInts".try_into()?,
            "serialized_add_two_ints",
        )?;

        // Capture the correct request/response metadata (derived from the service type support)
        // so the serialized callback can decode the request and encode the response. These are
        // the same types the service itself uses.
        let request_metadata = client.request_metadata().clone();
        let response_metadata = client.response_metadata().clone();

        // Serialized service: deserialize the request, add the operands, return a serialized
        // response.
        let _service = node.create_generic_serialized_service(
            "example_interfaces/srv/AddTwoInts".try_into()?,
            "serialized_add_two_ints",
            move |request_bytes: Vec<u8>| {
                let request = request_metadata.deserialize(&request_bytes).unwrap();
                let a = get_int64(&request, "a");
                let b = get_int64(&request, "b");
                let mut response = response_metadata.create().unwrap();
                set_int64(&mut response, "sum", a + b);
                response.serialize().unwrap()
            },
        )?;

        // Wait for the service to be discovered.
        let start = std::time::Instant::now();
        while !client.service_is_ready()? {
            executor.spin(SpinOptions::spin_once());
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
        }

        // Build a serialized request using the client's request metadata.
        let mut request = client.request_metadata().create()?;
        set_int64(&mut request, "a", 40);
        set_int64(&mut request, "b", 2);
        let request_bytes = request.serialize()?;

        // The blocking call must run off the executor spin thread.
        let result: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));
        let result_thread = Arc::clone(&result);
        let done_thread = Arc::clone(&done);
        let client_thread = Arc::clone(&client);
        let handle = std::thread::spawn(move || {
            let response = client_thread
                .call_serialized(&request_bytes, std::time::Duration::from_secs(10))
                .unwrap();
            *result_thread.lock().unwrap() = Some(response);
            done_thread.store(true, Ordering::Release);
        });

        let start = std::time::Instant::now();
        while !done.load(Ordering::Acquire) {
            executor.spin(SpinOptions::spin_once());
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
        }
        handle.join().unwrap();

        let response_bytes = result.lock().unwrap().take().unwrap();
        let response = client.response_metadata().deserialize(&response_bytes)?;
        assert_eq!(get_int64(&response, "sum"), 42);
        Ok(())
    }
}
