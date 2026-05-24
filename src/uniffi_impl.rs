use sans_io_time::Instant;
use uniffi::deps::anyhow::Error;

uniffi::custom_type!(Instant, i64, {
    remote,
    // Lowering the Rust Instant into a u64.
    lower: |instant| instant.as_nanos(),
    // Lifting the foreign u64 into the Rust Instant
    try_lift: |nanos| Result::<_, Error>::Ok(Instant::from_nanos(nanos)),
});
