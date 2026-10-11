use std::{sync::Arc, time::Duration};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        audio::AudioFrame,
        proto::audio::{
            depayloader::{AudioDepayloader, AudioDepayloaderConfig},
            payloader::{AudioPayloader, AudioPayloaderConfig},
        },
    },
};

#[test]
fn nofec() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    // Create Payloader
    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: false,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), first_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(5), second_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(10), third_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(15), fourth_data)
        .unwrap();

    // -- Test Depayloader
    // Create Depayloader
    let mut depayloader = AudioDepayloader::new(
        AudioDepayloaderConfig {
            encryption: None,
            fec: false,
        },
        Arc::new(DisabledCryptoBackend),
    );

    // Handle packets
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(0),
            buffer: first_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(5),
            buffer: second_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(10),
            buffer: third_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(15),
            buffer: fourth_data.to_vec(),
        })
    );
}

#[test]
fn nofec_reorder() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    // Create Payloader
    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: false,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), first_data)
        .unwrap();
    let first_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(5), second_data)
        .unwrap();
    let second_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(10), third_data)
        .unwrap();
    let third_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(15), fourth_data)
        .unwrap();
    let fourth_packet = payloader.poll_packet().unwrap();

    // -- Test Depayloader
    // Create Depayloader
    let mut depayloader = AudioDepayloader::new(
        AudioDepayloaderConfig {
            encryption: None,
            fec: false,
        },
        Arc::new(DisabledCryptoBackend),
    );

    // Handle packets
    depayloader.handle_packet(&fourth_packet).unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&second_packet).unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&first_packet).unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(0),
            buffer: first_data.to_vec(),
        })
    );
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(5),
            buffer: second_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&third_packet).unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(10),
            buffer: third_data.to_vec(),
        })
    );
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(15),
            buffer: fourth_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);
}

#[test]
fn nofec_loss_norecover() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];
    let fifth_data = &[8, 9, 10, 11];

    // Create Payloader
    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: false,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), first_data)
        .unwrap();
    let first_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(5), second_data)
        .unwrap();
    let second_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(10), third_data)
        .unwrap();
    let _third_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(15), fourth_data)
        .unwrap();
    let fourth_packet = payloader.poll_packet().unwrap();
    payloader
        .push_frame(Duration::from_millis(20), fifth_data)
        .unwrap();
    let fifth_packet = payloader.poll_packet().unwrap();

    // -- Test Depayloader
    // Create Depayloader
    let mut depayloader = AudioDepayloader::new(
        AudioDepayloaderConfig {
            encryption: None,
            fec: false,
        },
        Arc::new(DisabledCryptoBackend),
    );

    depayloader.handle_packet(&fourth_packet).unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&second_packet).unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&first_packet).unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(0),
            buffer: first_data.to_vec(),
        })
    );
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(5),
            buffer: second_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.try_skip_samples().unwrap();

    depayloader.handle_packet(&fourth_packet).unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(15),
            buffer: fourth_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader.handle_packet(&fifth_packet).unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(20),
            buffer: fifth_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);
}

#[test]
fn fec() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    // Create Payloader
    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: true,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), first_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(5), second_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(10), third_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(15), fourth_data)
        .unwrap();

    // -- Test Depayloader
    // Create Depayloader
    let mut depayloader = AudioDepayloader::new(
        AudioDepayloaderConfig {
            encryption: None,
            fec: false,
        },
        Arc::new(DisabledCryptoBackend),
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(0),
            buffer: first_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(5),
            buffer: second_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(10),
            buffer: third_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(15),
            buffer: fourth_data.to_vec(),
        })
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);
}

#[test]
fn fec_loss_recover() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    // Create Payloader
    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: true,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), first_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(5), second_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(10), third_data)
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(15), fourth_data)
        .unwrap();

    // -- Test Depayloader
    // Create Depayloader
    let mut depayloader = AudioDepayloader::new(
        AudioDepayloaderConfig {
            encryption: None,
            fec: true,
        },
        Arc::new(DisabledCryptoBackend),
    );

    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(0),
            buffer: first_data.to_vec(),
        })
    );

    // drop the second packet
    payloader.poll_packet().unwrap();

    // third
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    // fourth
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(depayloader.poll_frame().unwrap(), None);

    // fifth
    // reconstruct the loss with fec
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(5),
            buffer: second_data.to_vec(),
        })
    );
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(10),
            buffer: third_data.to_vec(),
        })
    );
    assert_eq!(
        depayloader.poll_frame().unwrap(),
        Some(AudioFrame {
            timestamp: Duration::from_millis(15),
            buffer: fourth_data.to_vec(),
        })
    );
    assert_eq!(depayloader.poll_frame().unwrap(), None);
}
