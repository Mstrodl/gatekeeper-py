use gatekeeper_core::{NfcError, NfcTag, UndifferentiatedTag};
use gatekeeper_members::{FetchError, GateKeeperMemberListener};
use pyo3::{exceptions::PyException, prelude::*};
use std::sync::mpsc;

#[pyclass(eq, eq_int)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TagType {
    Desfire,
    Mobile,
}

#[derive(Debug)]
#[pyclass]
struct Tag {
    tag_type: TagType,
    submit_scan_request: mpsc::SyncSender<mpsc::SyncSender<Result<String, NfcError>>>,
}

#[pymethods]
impl Tag {
    fn get_tag_type(&self) -> TagType {
        self.tag_type
    }

    fn get_association(&self) -> PyResult<String> {
        let (tx, rx) = mpsc::sync_channel(0);
        self.submit_scan_request
            .send(tx)
            .expect("Failed to submit scan request to worker?!");
        rx.recv()
            .expect("Failed to get scan results from worker?!")
            .map_err(|err| PyException::new_err(err.to_string()))
    }
}

enum ScanRequest {
    PollForTag(mpsc::SyncSender<Option<Tag>>),
    PollForAssociation(mpsc::SyncSender<Option<String>>),
    FetchUser(
        String,
        mpsc::SyncSender<Result<serde_json::Value, FetchError>>,
    ),
}

#[pyclass]
struct Reader {
    submit_scan_request: mpsc::Sender<ScanRequest>,
}

#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RealmType {
    Door = 0,
    Drink = 1,
    MemberProjects = 2,
}

impl From<RealmType> for gatekeeper_members::RealmType {
    fn from(realm_type: RealmType) -> Self {
        match realm_type {
            RealmType::Door => Self::Door,
            RealmType::Drink => Self::Drink,
            RealmType::MemberProjects => Self::MemberProjects,
        }
    }
}

impl From<gatekeeper_members::RealmType> for RealmType {
    fn from(realm_type: gatekeeper_members::RealmType) -> Self {
        match realm_type {
            gatekeeper_members::RealmType::Door => Self::Door,
            gatekeeper_members::RealmType::Drink => Self::Drink,
            gatekeeper_members::RealmType::MemberProjects => Self::MemberProjects,
        }
    }
}

impl Reader {
    fn scan_thread<'a>(
        mut reader: GateKeeperMemberListener<'a>,
        receiver: mpsc::Receiver<ScanRequest>,
    ) {
        loop {
            match receiver.recv() {
                Ok(ScanRequest::PollForTag(reply)) => {
                    let tag = match reader.poll_for_tag() {
                        Some(tag) => tag,
                        None => {
                            reply.send(None).ok();
                            continue;
                        }
                    };
                    let (tx, rx) = mpsc::sync_channel(0);
                    reply
                        .send(Some(Tag {
                            tag_type: match &tag {
                                UndifferentiatedTag::Desfire(_) => TagType::Desfire,
                                UndifferentiatedTag::Mobile(_) => TagType::Mobile,
                            },
                            submit_scan_request: tx,
                        }))
                        .ok();
                    if let Ok(reply) = rx.recv() {
                        reply.send(tag.authenticate()).ok();
                    }
                }
                Ok(ScanRequest::FetchUser(association_id, reply)) => {
                    reply.send(reader.fetch_user(association_id)).ok();
                }
                Ok(ScanRequest::PollForAssociation(reply)) => {
                    reply.send(reader.poll_for_user()).ok();
                }
                Err(_) => break,
            }
        }
    }
}

#[pymethods]
impl Reader {
    #[new]
    fn new(conn_string: String, realm_type: RealmType) -> Self {
        let (tx, rx) = mpsc::channel();
        let (tx_startup, rx_startup) = mpsc::sync_channel(0);
        let join_handle = std::thread::spawn(move || {
            let reader = GateKeeperMemberListener::new(conn_string, realm_type.into())
                .expect("Failed to init gatekeeper member listener");
            tx_startup.send(()).unwrap();
            Reader::scan_thread(reader, rx);
        });
        if rx_startup.recv().is_err() {
            join_handle
                .join()
                .expect("Failed to init gatekeeper member listener");
            panic!("Failed to init gatekeeper member listener");
        }
        Self {
            submit_scan_request: tx,
        }
    }

    fn poll_for_tag(&self) -> Option<Tag> {
        let (tx, rx) = mpsc::sync_channel(0);
        self.submit_scan_request
            .send(ScanRequest::PollForTag(tx))
            .expect("Couldn't send scan request to worker thread!");
        rx.recv().expect("Didn't get a reply from worker thread!")
    }

    fn poll_for_association(&self) -> Option<String> {
        let (tx, rx) = mpsc::sync_channel(0);
        self.submit_scan_request
            .send(ScanRequest::PollForAssociation(tx))
            .expect("Couldn't send scan request to worker thread!");
        rx.recv().expect("Didn't get a reply from worker thread!")
    }

    fn fetch_user(
        slf: PyRef<'_, Self>,
        association_id: String,
    ) -> Result<Option<Bound<'_, PyAny>>, PyErr> {
        let (tx, rx) = mpsc::sync_channel(0);
        slf.submit_scan_request
            .send(ScanRequest::FetchUser(association_id, tx))
            .expect("Couldn't send scan request to worker thread!");
        let user = rx
            .recv()
            .expect("Didn't get a reply from worker thread!")
            .map_err(|err| {
                PyException::new_err(match err {
                    FetchError::NetworkError => "Network Error".to_owned(),
                    FetchError::NotFound => "Not Found".to_owned(),
                    FetchError::ParseError => "Parse Error".to_owned(),
                    FetchError::Unknown => "Unknown".to_owned(),
                })
            })?;
        Ok(Some(
            pythonize::pythonize(slf.py(), &user).expect("Pythonize fail"),
        ))
    }
}

/// A Python module implemented in Rust.
#[pymodule]
mod gatekeeper_py {
    #[pymodule_export]
    use super::Reader;
    #[pymodule_export]
    use super::RealmType;
    #[pymodule_export]
    use super::Tag;
    #[pymodule_export]
    use super::TagType;
}
