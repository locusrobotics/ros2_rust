//! Vendored ROS 2 interface bindings (generated rosidl_generator_rs output).
//! Replaces the upstream build-time `ros-env` crate with committed sources so
//! rclrs is consumable as a plain cargo git dependency (no AMENT scan needed).
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::all)]
#![allow(unused_imports, missing_docs)]

pub mod builtin_interfaces {
    pub mod msg {
        include!("../msgs/builtin_interfaces/src/msg.rs");
        pub mod rmw {
            include!("../msgs/builtin_interfaces/src/msg/rmw.rs");
        }
    }
}

pub mod unique_identifier_msgs {
    pub mod msg {
        include!("../msgs/unique_identifier_msgs/src/msg.rs");
        pub mod rmw {
            include!("../msgs/unique_identifier_msgs/src/msg/rmw.rs");
        }
    }
}

pub mod service_msgs {
    pub mod msg {
        use crate::builtin_interfaces;
        include!("../msgs/service_msgs/src/msg.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            include!("../msgs/service_msgs/src/msg/rmw.rs");
        }
    }
}

pub mod rcl_interfaces {
    pub mod msg {
        use crate::builtin_interfaces;
        use crate::service_msgs;
        include!("../msgs/rcl_interfaces/src/msg.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            use crate::service_msgs;
            include!("../msgs/rcl_interfaces/src/msg/rmw.rs");
        }
    }
    pub mod srv {
        use crate::builtin_interfaces;
        use crate::service_msgs;
        include!("../msgs/rcl_interfaces/src/srv.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            use crate::service_msgs;
            include!("../msgs/rcl_interfaces/src/srv/rmw.rs");
        }
    }
}

pub mod action_msgs {
    pub mod msg {
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/action_msgs/src/msg.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/action_msgs/src/msg/rmw.rs");
        }
    }
    pub mod srv {
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/action_msgs/src/srv.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/action_msgs/src/srv/rmw.rs");
        }
    }
}

pub mod rosgraph_msgs {
    pub mod msg {
        use crate::builtin_interfaces;
        use crate::rcl_interfaces;
        use crate::service_msgs;
        include!("../msgs/rosgraph_msgs/src/msg.rs");
        pub mod rmw {
            use crate::builtin_interfaces;
            use crate::rcl_interfaces;
            use crate::service_msgs;
            include!("../msgs/rosgraph_msgs/src/msg/rmw.rs");
        }
    }
}

pub mod test_msgs {
    pub mod msg {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/test_msgs/src/msg.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/test_msgs/src/msg/rmw.rs");
        }
    }
    pub mod srv {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/test_msgs/src/srv.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/test_msgs/src/srv/rmw.rs");
        }
    }
    pub mod action {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/test_msgs/src/action.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/test_msgs/src/action/rmw.rs");
        }
    }
}

pub mod example_interfaces {
    pub mod msg {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/example_interfaces/src/msg.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/example_interfaces/src/msg/rmw.rs");
        }
    }
    pub mod srv {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/example_interfaces/src/srv.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/example_interfaces/src/srv/rmw.rs");
        }
    }
    pub mod action {
        use crate::action_msgs;
        use crate::builtin_interfaces;
        use crate::service_msgs;
        use crate::unique_identifier_msgs;
        include!("../msgs/example_interfaces/src/action.rs");
        pub mod rmw {
            use crate::action_msgs;
            use crate::builtin_interfaces;
            use crate::service_msgs;
            use crate::unique_identifier_msgs;
            include!("../msgs/example_interfaces/src/action/rmw.rs");
        }
    }
}

