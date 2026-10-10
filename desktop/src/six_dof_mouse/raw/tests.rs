use super::*;
use std::sync::mpsc;

fn captured() -> (SixDofEventSink, Arc<Mutex<Vec<SixDofEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let output = events.clone();
    (
        Arc::new(move |event| output.lock().unwrap().push(event)),
        events,
    )
}

fn axes(id: u8, values: &[i16]) -> Vec<u8> {
    let mut report = vec![id];
    report.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    report
}

#[test]
fn raw_reports_preserve_signed_axes_and_coalesce_latest_split_updates() {
    let (sink, events) = captured();
    let now = Instant::now();
    let mut reports = Reports::new(now);
    reports.receive(&axes(1, &[i16::MIN, 350, -42, i16::MAX, -350, 0]), &sink);
    reports.flush(|| now, &sink);
    reports.receive(&axes(1, &[1, 2, 3]), &sink);
    reports.receive(&axes(2, &[4, 5, 6]), &sink);
    reports.receive(&axes(1, &[7, 8, 9]), &sink);
    reports.flush(|| now + Duration::from_millis(15), &sink);
    assert_eq!(events.lock().unwrap().len(), 1);
    reports.flush(|| now + Duration::from_millis(16), &sink);
    reports.receive(&axes(2, &[0, 0, 0]), &sink);
    reports.flush(|| now + Duration::from_millis(32), &sink);
    reports.flush(|| now + Duration::from_millis(48), &sink);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            SixDofEvent::Motion(MotionPacket {
                translation: Some([i16::MIN, 350, -42]),
                rotation: Some([i16::MAX, -350, 0]),
            }),
            SixDofEvent::Motion(MotionPacket {
                translation: Some([7, 8, 9]),
                rotation: Some([4, 5, 6])
            }),
            SixDofEvent::Motion(MotionPacket {
                translation: None,
                rotation: Some([0, 0, 0])
            }),
        ]
    );
}

#[test]
fn short_or_unknown_motion_reports_do_not_replace_pending_axes() {
    let (sink, events) = captured();
    let now = Instant::now();
    let mut reports = Reports::new(now);
    reports.receive(&axes(1, &[10, 20, 30, 40, 50, 60]), &sink);
    for report in [
        vec![],
        vec![1],
        vec![1, 9, 0],
        axes(2, &[99, 100]),
        axes(99, &[1, 2, 3]),
    ] {
        reports.receive(&report, &sink);
    }
    reports.flush(|| now, &sink);
    assert_eq!(
        *events.lock().unwrap(),
        vec![SixDofEvent::Motion(MotionPacket {
            translation: Some([10, 20, 30]),
            rotation: Some([40, 50, 60]),
        })]
    );
}

#[test]
fn buttons_are_immediate_rising_edges_in_one_based_order() {
    let (sink, events) = captured();
    let mut reports = Reports::new(Instant::now());
    for report in [
        &[3, 5, 0, 0, 128][..],
        &[3, 5, 0, 0, 128],
        &[3, 0],
        &[3, 1],
        &[3],
    ] {
        reports.receive(report, &sink);
    }
    assert_eq!(
        *events.lock().unwrap(),
        [1, 3, 32, 1]
            .into_iter()
            .map(|button| SixDofEvent::Button(ButtonPacket { button }))
            .collect::<Vec<_>>()
    );
}

#[test]
fn worker_uses_the_existing_read_timeout_and_emits_one_error_before_stopping() {
    let (send, receive) = mpsc::channel();
    let mut input = [
        Ok(axes(1, &[1, -2, 3])),
        Ok(vec![3, 1]),
        Err("unplugged".to_owned()),
    ]
    .into_iter();
    let worker = spawn(
        move |buffer, timeout| {
            assert_eq!(timeout, 8);
            assert_eq!(buffer.len(), 64);
            let report = input.next().expect("read after terminal device error")?;
            buffer[..report.len()].copy_from_slice(&report);
            Ok(report.len())
        },
        Arc::new(move |event| {
            send.send(event).unwrap();
        }),
    )
    .unwrap();
    let actual = (0..3)
        .map(|_| receive.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<Vec<_>>();
    drop(worker);
    assert_eq!(
        actual,
        vec![
            SixDofEvent::Motion(MotionPacket {
                translation: Some([1, -2, 3]),
                rotation: None
            }),
            SixDofEvent::Button(ButtonPacket { button: 1 }),
            SixDofEvent::Error("unplugged".into()),
        ]
    );
    assert_eq!(receive.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}
