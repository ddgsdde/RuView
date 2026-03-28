"""RuView WiFi Sensor — Home Assistant integration.

This integration connects to a running RuView sensing server
(wifi-densepose-sensing-server) and exposes its data as Home Assistant
entities:

  binary_sensor.ruview_presence     — occupancy detection
  binary_sensor.ruview_fall         — fall alert
  sensor.ruview_breathing_rate      — breathing rate (BPM)
  sensor.ruview_heart_rate          — heart rate (BPM)
  sensor.ruview_person_count        — estimated person count
  sensor.ruview_motion_level        — absent / present_still / active

Setup via the UI:
  Settings → Devices & Services → Add Integration → RuView WiFi Sensor
"""

from __future__ import annotations

import asyncio
import logging
from datetime import timedelta

import aiohttp

from homeassistant.config_entries import ConfigEntry
from homeassistant.const import Platform
from homeassistant.core import HomeAssistant
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.update_coordinator import DataUpdateCoordinator, UpdateFailed

from .const import (
    CONF_HOST,
    CONF_PORT,
    CONF_SCAN_INTERVAL,
    DEFAULT_SCAN_INTERVAL,
    DOMAIN,
    ENDPOINT_LATEST,
    ENDPOINT_VITALS,
)

_LOGGER = logging.getLogger(__name__)

PLATFORMS: list[Platform] = [Platform.BINARY_SENSOR, Platform.SENSOR]


async def async_setup_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    """Set up RuView from a config entry."""
    host: str = entry.data[CONF_HOST]
    port: int = entry.data[CONF_PORT]
    scan_interval: int = entry.data.get(CONF_SCAN_INTERVAL, DEFAULT_SCAN_INTERVAL)

    session = async_get_clientsession(hass)
    base_url = f"http://{host}:{port}"

    async def _fetch_data() -> dict:
        try:
            async with session.get(
                f"{base_url}{ENDPOINT_LATEST}", timeout=aiohttp.ClientTimeout(total=5)
            ) as resp:
                resp.raise_for_status()
                sensing = await resp.json()

            async with session.get(
                f"{base_url}{ENDPOINT_VITALS}", timeout=aiohttp.ClientTimeout(total=5)
            ) as resp:
                resp.raise_for_status()
                vitals = await resp.json()

            return {"sensing": sensing, "vitals": vitals}
        except (aiohttp.ClientError, asyncio.TimeoutError) as err:
            raise UpdateFailed(
                f"Connection error communicating with RuView at {base_url}: {err}"
            ) from err
        except aiohttp.ClientResponseError as err:
            raise UpdateFailed(
                f"HTTP {err.status} from RuView at {base_url}: {err.message}"
            ) from err

    coordinator = DataUpdateCoordinator(
        hass,
        _LOGGER,
        name=DOMAIN,
        update_method=_fetch_data,
        update_interval=timedelta(seconds=scan_interval),
    )

    await coordinator.async_config_entry_first_refresh()

    hass.data.setdefault(DOMAIN, {})[entry.entry_id] = coordinator

    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)
    return True


async def async_unload_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    """Unload a config entry."""
    if unload_ok := await hass.config_entries.async_unload_platforms(entry, PLATFORMS):
        hass.data[DOMAIN].pop(entry.entry_id)
    return unload_ok
