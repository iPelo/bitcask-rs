# Durability

This document will define the storage guarantees precisely.

Initial policy:

- Buffered mode may lose recent acknowledged writes if the process or machine
  crashes before the operating system flushes file buffers.
- Sync-on-write mode should call `fsync` before acknowledging a write.
- Recovery should tolerate a torn record at the end of the active file by
  truncating back to the last valid record.

