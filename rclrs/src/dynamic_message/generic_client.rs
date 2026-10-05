use std::{
    any::Any,
    boxed::Box,
    collections::HashMap,
    ffi::CString,
    sync::{
        mpsc::{channel, Sender},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};

use super::{
    get_service_type_support_handle, get_type_support_library, DynamicMessage,
    DynamicMessageMetadata, MessageTypeName, ServiceTypeName,
    INTROSPECTION_TYPE_SUPPORT_IDENTIFIER, REGULAR_TYPE_SUPPORT_IDENTIFIER,
};
use crate::{
    error::ToResult, rcl_bindings::*, ClientOptions, NodeHandle, RclPrimitive, RclPrimitiveHandle,
    RclPrimitiveKind, RclReturnCode, RclrsError, ReadyKind, Waitable, WaitableLifecycle,
    WorkerCommands, ENTITY_LIFECYCLE_MUTEX,
};

/// A service client whose request/response types are only known at runtime.
///
/// Create one using [`NodeState::create_generic_client`][1].
///
/// [1]: crate::NodeState::create_generic_client
pub type GenericClient = Arc<GenericClientState>;

type SequenceNumber = i64;

/// The inner state of a [`GenericClient`].
pub struct GenericClientState {
    handle: Arc<GenericClientHandle>,
    board: Arc<Mutex<HashMap<SequenceNumber, Sender<DynamicMessage>>>>,
    #[allow(unused)]
    lifecycle: WaitableLifecycle,
    request_metadata: Arc<DynamicMessageMetadata>,
    #[allow(dead_code)]
    response_metadata: Arc<DynamicMessageMetadata>,
    #[allow(dead_code)]
    type_support_library: Arc<libloading::Library>,
}

impl GenericClientState {
    /// Returns the name of the service this client calls, after remapping.
    pub fn service_name(&self) -> String {
        // SAFETY: The client handle is kept valid by its Arc.
        unsafe {
            let char_ptr = rcl_client_get_service_name(&*self.handle.lock());
            debug_assert!(!char_ptr.is_null());
            std::ffi::CStr::from_ptr(char_ptr)
                .to_string_lossy()
                .into_owned()
        }
    }

    /// Returns true if a service server matching this client is available.
    pub fn service_is_ready(&self) -> Result<bool, RclrsError> {
        let mut is_ready = false;
        let client = &*self.handle.lock();
        let node = &*self.handle.node_handle.rcl_node.lock().unwrap();
        unsafe {
            // SAFETY: Both node and client are valid, and the client was created from the node.
            rcl_service_server_is_available(node as *const _, client as *const _, &mut is_ready)
        }
        .ok()?;
        Ok(is_ready)
    }

    /// Sends a request and blocks until the response arrives or the timeout elapses.
    ///
    /// This must NOT be called from the executor spin thread, since it blocks waiting for the
    /// executor to route the response. It is intended to be called from a separate worker thread.
    pub fn call(
        &self,
        request: DynamicMessage,
        timeout: Duration,
    ) -> Result<DynamicMessage, RclrsError> {
        let (tx, rx) = channel();
        let mut sequence_number: SequenceNumber = -1;
        unsafe {
            // SAFETY: The client handle ensures the rcl_client is valid; the request was built
            // from the matching request metadata.
            rcl_send_request(
                &*self.handle.lock() as *const _,
                request.storage.as_ptr() as *mut _,
                &mut sequence_number,
            )
        }
        .ok()?;

        self.board
            .lock()
            .map_err(|_| RclrsError::PoisonedMutex)?
            .insert(sequence_number, tx);

        rx.recv_timeout(timeout).map_err(|_| {
            // Clean up the pending entry on timeout.
            if let Ok(mut board) = self.board.lock() {
                board.remove(&sequence_number);
            }
            RclrsError::RclError {
                code: RclReturnCode::Timeout,
                msg: None,
            }
        })
    }

    /// Sends a serialized request and blocks until the serialized response arrives or the
    /// timeout elapses.
    ///
    /// The `request_bytes` must be the CDR serialization of this service's request message. The
    /// returned bytes are the CDR serialization of the response message. Like [`call`], this must
    /// NOT be called from the executor spin thread.
    ///
    /// [`call`]: Self::call
    pub fn call_serialized(
        &self,
        request_bytes: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, RclrsError> {
        let request = self.request_metadata.deserialize(request_bytes)?;
        let response = self.call(request, timeout)?;
        Ok(self.response_metadata.serialize(&response)?)
    }

    /// Builds a request message for this client's service type.
    pub fn create_request(&self) -> Result<DynamicMessage, RclrsError> {
        Ok(self.request_metadata.create()?)
    }

    /// Returns the metadata describing this client's request message type.
    pub fn request_metadata(&self) -> &DynamicMessageMetadata {
        &self.request_metadata
    }

    /// Returns the metadata describing this client's response message type.
    pub fn response_metadata(&self) -> &DynamicMessageMetadata {
        &self.response_metadata
    }

    /// Creates a new generic client.
    pub(crate) fn create<'a>(
        service_type: ServiceTypeName,
        options: impl Into<ClientOptions<'a>>,
        node_handle: &Arc<NodeHandle>,
        commands: &Arc<WorkerCommands>,
    ) -> Result<Arc<Self>, RclrsError> {
        let ClientOptions { service_name, qos } = options.into();

        // Build the request/response metadata from the introspection service type support.
        let introspection_library = get_type_support_library(
            &service_type.package_name,
            INTROSPECTION_TYPE_SUPPORT_IDENTIFIER,
        )?;
        // SAFETY: The symbol type is trusted assuming the install dir hasn't been tampered with.
        let service_type_support_ptr = unsafe {
            get_service_type_support_handle(
                introspection_library.as_ref(),
                INTROSPECTION_TYPE_SUPPORT_IDENTIFIER,
                &service_type,
            )?
        };
        // SAFETY: The pointer is valid while the library is loaded.
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

        // Load the regular type support handle used to initialize the rcl client.
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

        let name_c_string =
            CString::new(service_name).map_err(|err| RclrsError::StringContainsNul {
                err,
                s: service_name.into(),
            })?;

        // SAFETY: No preconditions for this function.
        let mut client_options = unsafe { rcl_client_get_default_options() };
        client_options.qos = qos.into();
        // SAFETY: Getting a zero-initialized value is always safe.
        let mut rcl_client = unsafe { rcl_get_zero_initialized_client() };
        {
            let rcl_node = node_handle.rcl_node.lock().unwrap();
            let _lifecycle_lock = ENTITY_LIFECYCLE_MUTEX.lock().unwrap();
            unsafe {
                // SAFETY:
                // * The rcl_client is zero-initialized as mandated by this function.
                // * The rcl_node is kept alive by the NodeHandle.
                // * The name and options are copied by this function.
                // * The entity lifecycle mutex is locked to protect rmw global state.
                rcl_client_init(
                    &mut rcl_client,
                    &*rcl_node,
                    type_support,
                    name_c_string.as_ptr(),
                    &client_options,
                )
                .ok()?;
            }
        }

        let handle = Arc::new(GenericClientHandle {
            rcl_client: Mutex::new(rcl_client),
            node_handle: Arc::clone(node_handle),
        });

        let board = Arc::new(Mutex::new(HashMap::new()));
        let request_metadata = Arc::new(request_metadata);
        let response_metadata = Arc::new(response_metadata);

        let (waitable, lifecycle) = Waitable::new(
            Box::new(GenericClientExecutable {
                handle: Arc::clone(&handle),
                board: Arc::clone(&board),
                response_metadata: Arc::clone(&response_metadata),
            }),
            Some(Arc::clone(commands.get_guard_condition())),
        );
        commands.add_to_wait_set(waitable);

        Ok(Arc::new(Self {
            handle,
            board,
            lifecycle,
            request_metadata,
            response_metadata,
            type_support_library,
        }))
    }
}

struct GenericClientExecutable {
    handle: Arc<GenericClientHandle>,
    board: Arc<Mutex<HashMap<SequenceNumber, Sender<DynamicMessage>>>>,
    response_metadata: Arc<DynamicMessageMetadata>,
}

impl RclPrimitive for GenericClientExecutable {
    unsafe fn execute(
        &mut self,
        ready: ReadyKind,
        _payload: &mut dyn Any,
    ) -> Result<(), RclrsError> {
        ready.for_basic()?;

        let mut response = self.response_metadata.create()?;
        // SAFETY: A zeroed rmw_service_info_t is a valid value.
        let mut service_info = std::mem::zeroed::<rmw_service_info_t>();
        let response_ptr = response.storage.as_mut_ptr();
        let take_result = {
            // SAFETY: All three pointers are valid and kept alive by the handle/response.
            rcl_take_response_with_info(
                &*self.handle.lock(),
                &mut service_info,
                response_ptr as *mut _,
            )
            .ok()
        };
        match take_result {
            Ok(()) => {}
            Err(RclrsError::RclError {
                code: RclReturnCode::ClientTakeFailed,
                ..
            }) => {
                // Spurious wakeup, nothing to do.
                return Ok(());
            }
            Err(err) => return Err(err),
        }

        let seq = service_info.request_id.sequence_number;
        if let Some(sender) = self.board.lock().unwrap().remove(&seq) {
            // The receiver may have timed out and gone away; ignore send errors.
            let _ = sender.send(response);
        }
        Ok(())
    }

    fn kind(&self) -> RclPrimitiveKind {
        RclPrimitiveKind::Client
    }

    fn handle(&self) -> RclPrimitiveHandle<'_> {
        RclPrimitiveHandle::Client(self.handle.lock())
    }
}

/// Manage the lifecycle of an `rcl_client_t` for a generic client.
struct GenericClientHandle {
    rcl_client: Mutex<rcl_client_t>,
    node_handle: Arc<NodeHandle>,
}

impl GenericClientHandle {
    fn lock(&self) -> MutexGuard<'_, rcl_client_t> {
        self.rcl_client.lock().unwrap()
    }
}

impl Drop for GenericClientHandle {
    fn drop(&mut self) {
        let rcl_client = self.rcl_client.get_mut().unwrap();
        let mut rcl_node = self.node_handle.rcl_node.lock().unwrap();
        let _lifecycle_lock = ENTITY_LIFECYCLE_MUTEX.lock().unwrap();
        // SAFETY: The entity lifecycle mutex is locked to protect rmw global state.
        unsafe {
            rcl_client_fini(rcl_client, &mut *rcl_node);
        }
    }
}
