/// Retain whole records in stored order, including their padding. Leave the
/// header before `start` and any partial record at the end intact.
pub(crate) fn retain_records(
    bytes: &mut Vec<u8>,
    start: usize,
    stride: usize,
    mut keep: impl FnMut(&[u8]) -> bool,
) {
    let tail_len = (bytes.len() - start) % stride;
    let end = bytes.len() - tail_len;
    let mut write = start;
    for read in (start..end).step_by(stride) {
        let record = read..read + stride;
        if keep(&bytes[record.clone()]) {
            if write != read {
                bytes.copy_within(record, write);
            }
            write += stride;
        }
    }
    if write != end {
        bytes.copy_within(end.., write);
        bytes.truncate(write + tail_len);
    }
}
