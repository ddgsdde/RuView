"""Constants for the RuView WiFi Sensor integration."""

DOMAIN = "ruview"

DEFAULT_HOST = "localhost"
DEFAULT_PORT = 8080
DEFAULT_SCAN_INTERVAL = 2  # seconds

CONF_HOST = "host"
CONF_PORT = "port"
CONF_SCAN_INTERVAL = "scan_interval"

ENDPOINT_LATEST = "/api/v1/sensing/latest"
ENDPOINT_VITALS = "/api/v1/vital-signs"
ENDPOINT_HEALTH = "/health"
