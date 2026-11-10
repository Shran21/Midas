ALTER TABLE midas.devices
    ADD COLUMN unattended_window_start bigint,
    ADD COLUMN unattended_window_end bigint,
    ADD CONSTRAINT unattended_window_valid CHECK (
        (unattended_window_start IS NULL AND unattended_window_end IS NULL)
        OR (unattended_window_start IS NOT NULL AND unattended_window_end IS NOT NULL
            AND unattended_window_start >= 0
            AND unattended_window_end > unattended_window_start
            AND unattended_window_end <= 253402300799)
    );
