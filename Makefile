.PHONY: help hot-update hot-update-legacy build-dashboard presubmit verify-local

help:
	@echo "Available targets:"
	@echo "  make hot-update           # Python: local build + push to remote containers (see AGENTS.md)"
	@echo "  make hot-update-legacy    # Bash hot_update_runtime.sh (local DOCKER_HOST required)"
	@echo "  make build-dashboard      # Build crab-dashboard static dist assets"
	@echo "  make presubmit            # Run local fmt/clippy/test/deny/dashboard checks"
	@echo "  make verify-local         # Run local deployment smoke checks"

hot-update:
	@python3 scripts/hot_update.py

hot-update-legacy:
	@./scripts/hot_update_runtime.sh

build-dashboard:
	@./scripts/build_dashboard.sh

presubmit:
	@./scripts/presubmit.sh

verify-local:
	@./scripts/verify_local_integration.sh
