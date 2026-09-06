CREATE INDEX check_run_target_check_finished_id_idx ON health_monitor.check_run (check_run_target, check_run_check, check_run_finished_at DESC, check_run_id DESC);
