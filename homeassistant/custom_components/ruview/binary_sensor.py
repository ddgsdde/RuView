"""Binary sensors for the RuView WiFi Sensor integration."""

from __future__ import annotations

from homeassistant.components.binary_sensor import (
    BinarySensorDeviceClass,
    BinarySensorEntity,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback
from homeassistant.helpers.update_coordinator import CoordinatorEntity, DataUpdateCoordinator

from .const import DOMAIN


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    """Set up RuView binary sensors."""
    coordinator: DataUpdateCoordinator = hass.data[DOMAIN][entry.entry_id]

    async_add_entities(
        [
            RuViewPresenceSensor(coordinator, entry),
            RuViewFallSensor(coordinator, entry),
        ]
    )


class _RuViewBinarySensorBase(CoordinatorEntity, BinarySensorEntity):
    """Base class for RuView binary sensors."""

    def __init__(
        self,
        coordinator: DataUpdateCoordinator,
        entry: ConfigEntry,
        key: str,
        name: str,
        device_class: BinarySensorDeviceClass | None,
    ) -> None:
        super().__init__(coordinator)
        self._key = key
        self._attr_name = name
        self._attr_device_class = device_class
        self._attr_unique_id = f"{entry.entry_id}_{key}"
        self._attr_device_info = {
            "identifiers": {(DOMAIN, entry.entry_id)},
            "name": f"RuView ({entry.data['host']}:{entry.data['port']})",
            "model": "ESP32-S3 WiFi CSI Node",
            "manufacturer": "RuView",
        }


class RuViewPresenceSensor(_RuViewBinarySensorBase):
    """Occupancy binary sensor."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(
            coordinator,
            entry,
            key="presence",
            name="RuView Presence",
            device_class=BinarySensorDeviceClass.OCCUPANCY,
        )

    @property
    def is_on(self) -> bool | None:
        data = self.coordinator.data
        if data is None:
            return None
        return bool(
            data.get("sensing", {})
            .get("classification", {})
            .get("presence", False)
        )


class RuViewFallSensor(_RuViewBinarySensorBase):
    """Fall detection binary sensor."""

    def __init__(self, coordinator: DataUpdateCoordinator, entry: ConfigEntry) -> None:
        super().__init__(
            coordinator,
            entry,
            key="fall_detected",
            name="RuView Fall Detected",
            device_class=BinarySensorDeviceClass.SAFETY,
        )

    @property
    def is_on(self) -> bool | None:
        data = self.coordinator.data
        if data is None:
            return None
        edge = data.get("sensing", {}).get("edge_vitals")
        if edge:
            return bool(edge.get("fall_detected", False))
        return False
