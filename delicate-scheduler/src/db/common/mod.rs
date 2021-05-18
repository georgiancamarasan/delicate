pub(crate) mod state;

pub(crate) mod types {
    use super::state::task_log::State;
    #[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub(crate) enum EventType {
        TaskPerform = 1,
        TaskFinish = 2,
        TaskTimeout = 3,
        Unknown = 81,
    }

    impl From<i16> for EventType {
        fn from(value: i16) -> Self {
            match value {
                1 => EventType::TaskPerform,
                2 => EventType::TaskFinish,
                3 => EventType::TaskTimeout,
                _ => EventType::Unknown,
            }
        }
    }

    impl From<EventType> for State {
        fn from(value: EventType) -> Self {
            match value {
                EventType::TaskPerform => State::Running,
                EventType::TaskFinish => State::NormalEnding,
                EventType::TaskTimeout => State::TimeoutEnding,
                EventType::Unknown => State::Unknown,
            }
        }
    }
}
