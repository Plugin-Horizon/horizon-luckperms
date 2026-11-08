// Orderly stop: the worker finishes its queue.
fn on_stop() {
    horizon_luckperms_core::glue::stop();
}
