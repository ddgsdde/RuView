"""Sensors for the RuView WiFi Sensor integration."""

from __future__ import annotations

from homeassistant.components.sensor import (
    SensorDeviceClass,
    SensorEntity,
    SensorStateClass,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.const import UnitOfFrequency
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback
from homeassistant.helpers.update_coordinator import CoordinatorEntity, DataUpdateCoordinator

from .const import DOMAIN

# UnitOfFrequency.HERTZ is "Hz"; for BPM we use a plain string.
UNIT_BPM = "BPM"
UNIT_PERSONS = "persons"
UNIT_DBM = "dBm"


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    """Set up RuView sensors."""
    coordinator: DataUpdateCoordinator = hass.data[DOMAIN][entry.entry_id]

    async_add_entities(
        [
            RuViewBreathingRateSensor(coordinator, entry),
            RuViewHeartRateSensor(coordinator, entry),
            RuViewPersonCountSensor(coordinator, entry),
            RuViewMotionLevelSensor(coordinator, entry),
            RuViewRssiSensor(coordinator, entry),
        ]
    )


class _RuViewSensorBase(CoordinatorEntity, SensorEntity):
    """Base class for RuView sensors."""

    def __init__(
        self,
        coordinator: DataUpdateCoordinator,
        entry: ConfigEntry,
        key: str,
        name: str,
    ) -> None:
        super().__init__(coordinator)
        self._key = key
        self._attr_name = name
        self._attr_unique_id = f"{entry.entry_id}_{key}"
        self._attr_device_info = {
            "identifiers": {(DOMAIN, entry.entry_id)},
            "name": f"RuView ({entry.data['host']}:{entry.data['port']})",
            "model": "ESP32-S3 WiFi CSI Node",
            "manufacturer": "RuView",
        }


class RuViewBreathingRateSensor(_RuViewSensorBase):
    """Breathing rate sensor (BPM)."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(coordinator, entry, "breathing_rate_bpm", "RuView Breathing Rate")
        self._attr_native_unit_of_measurement = UNIT_BPM
        self._attr_state_class = SensorStateClass.MEASUREMENT
        self._attr_icon = "mdi:lungs"

    @property
    def native_value(self) -> float | None:
        data = self.coordinator.data
        if data is None:
            return None
        return data.get("vitals", {}).get("breathing_rate_bpm")


class RuViewHeartRateSensor(_RuViewSensorBase):
    """Heart rate sensor (BPM)."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(coordinator, entry, "heart_rate_bpm", "RuView Heart Rate")
        self._attr_native_unit_of_measurement = UNIT_BPM
        self._attr_state_class = SensorStateClass.MEASUREMENT
        self._attr_device_class = SensorDeviceClass.HEART_RATE

    @property
    def native_value(self) -> float | None:
        data = self.coordinator.data
        if data is None:
            return None
        return data.get("vitals", {}).get("heart_rate_bpm")


class RuViewPersonCountSensor(_RuViewSensorBase):
    """Estimated person count sensor."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(coordinator, entry, "person_count", "RuView Person Count")
        self._attr_native_unit_of_measurement = UNIT_PERSONS
        self._attr_state_class = SensorStateClass.MEASUREMENT
        self._attr_icon = "mdi:account-multiple"

    @property
    def native_value(self) -> int | None:
        data = self.coordinator.data
        if data is None:
            return None
        sensing = data.get("sensing", {})
        return sensing.get("estimated_persons") or 0


class RuViewMotionLevelSensor(_RuViewSensorBase):
    """Motion level sensor (absent / present_still / active)."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(coordinator, entry, "motion_level", "RuView Motion Level")
        self._attr_icon = "mdi:motion-sensor"

    @property
    def native_value(self) -> str | None:
        data = self.coordinator.data
        if data is None:
            return None
        return (
            data.get("sensing", {})
            .get("classification", {})
            .get("motion_level", "absent")
        )


class RuViewRssiSensor(_RuViewSensorBase):
    """WiFi RSSI signal strength sensor."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(coordinator, entry, "rssi", "RuView WiFi Signal")
        self._attr_native_unit_of_measurement = UNIT_DBM
        self._attr_state_class = SensorStateClass.MEASUREMENT
        self._attr_device_class = SensorDeviceClass.SIGNAL_STRENGTH
        self._attr_entity_category = "diagnostic"

    @property
    def native_value(self) -> float | None:
        data = self.coordinator.data
        if data is None:
            return None
        nodes = data.get("sensing", {}).get("nodes", [])
        if nodes:
            return nodes[0].get("rssi_dbm")
        return None
