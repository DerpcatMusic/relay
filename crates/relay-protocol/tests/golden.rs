use prost::Message;
use relay_protocol::v1::{Envelope, envelope, hello};

const HELLO_RESUME_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/protocol/hello-resume-v1.bin");

#[test]
fn rust_decodes_and_reencodes_the_hello_resume_golden_fixture() {
    let envelope = Envelope::decode(HELLO_RESUME_V1).expect("golden fixture must decode");

    assert_eq!(
        envelope.version.as_ref().map(|version| version.major),
        Some(1)
    );
    assert_eq!(envelope.revision, 42);

    let Some(envelope::Payload::Hello(hello_message)) = &envelope.payload else {
        panic!("golden fixture must contain a Hello payload");
    };
    let Some(hello::Entry::Resume(resume)) = &hello_message.entry else {
        panic!("golden fixture must contain a Resume entry");
    };
    assert_eq!(resume.last_seen_revision, 41);
    assert_eq!(envelope.encode_to_vec(), HELLO_RESUME_V1);
}
