
<div align="center">

# 📡 CSI Lite-TUI
## A Handheld Scope for the Invisible World

> Real-time Wi-Fi Channel State Information (CSI) visualization built for the M5Stack CoreS3 SE.

<br>

<!-- Displaying the Hardware and Interface side-by-side -->
<img src="https://github.com/joverse962/Images/blob/main/LiteTUI/coreS3.webp" alt="M5Stack CoreS3 SE Device" width="35%">
&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
<img src="https://github.com/joverse962/Images/blob/main/LiteTUI/welcome3.png" alt="Blind-Cam Hero View" width="35%">

<br><br>
<video src="https://github.com/joverse962/Images/blob/main/LiteTUI/amplitude.mov" autoplay loop muted playsinline></video>

---
</div>

## 🛠️ Supported Hardware

This lightweight terminal user interface (TUI) is strictly optimized for the **[M5Stack CoreS3 SE IoT Controller](https://shop.m5stack.com/products/m5stack-cores3-se-iot-controller-w-o-battery-bottom)**. Its compact form factor, physical touch interface, and robust ESP32 processing power make it the ultimate vessel for a portable spectral analyzer.

---

## 📖 Operational Manual & Interface Guide

<br>
<video src="https://github.com/joverse962/Images/blob/main/LiteTUI/amplitude.mov" autoplay loop muted playsinline></video>
<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>🟢 1. The Boot Sequence</h3>
      <p>Upon powering up the CoreS3 SE, the system initiates the neon-styled retro TUI. Designed for high-visibility in field operations, the interface carries zero bloat. Tap <b>START VISUALIZING</b> to initialize the radio and memory allocation.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/IMG_7949.jpg" alt="Welcome Screen" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>⚙️ 2. Deployment Modes</h3>
      <p>Adapt to your environment by selecting the appropriate connection topology:</p>
      <ul>
        <li><b>STATION MODE</b></li>
        <li><b>SNIFFER MODE</b></li>
        <li><b>ESP-NOW MODE</b></li>
      </ul>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/IMG_7952.jpg" alt="Modes Screen" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>📈 3. Amplitude (Packets/Sec)</h3>
      <p>Visualize the raw volume of data moving through the airwaves. This real-time graph maps amplitude across subcarriers, instantly highlighting network congestion, heavy data transfers, or high-bandwidth anomalies in your immediate vicinity.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/amplitude.gif" alt="Amplitude View" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>〰️ 4. Unwrapped Phase Offset</h3>
      <p>Strip away the noise and look at the physical properties of the signal. By tracking the unwrapped phase offset, you can detect microscopic environmental shifts, physical movement within a room, and complex multipath fading effects.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/phase.gif" alt="Phase View" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>🔥 5. Spectral Waterfall</h3>
      <p>A scrolling matrix mapping amplitude history over time into a density color-map. This view acts as a "heat map" for the air, allowing you to track fleeting signal bursts, frequency hopping, and persistent noise floors as they happen across the spectrum.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/waterfall2.png" alt="Waterfall View" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>📡 6. Signal Strength (RSSI) & EMA</h3>
      <p>Track the aggregate power of your selected target. This graph plots signal strength (in dBm) over time. It utilizes an <i>Exponential Moving Average (EMA)</i> to smooth out erratic environmental spikes, providing a clear directional trend for tracking.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/rssi2.png" alt="RSSI View" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>💾 7. Edge Data Logging</h3>
      <p>Don't lose your scan data. Before deployment, the TUI prompts you to initialize the onboard SD card. Capturing raw CSI data as CSV files allows you to extract the payload later for advanced filtering and machine-learning analysis on your main machine.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/csv.jpg" alt="SD Card Save Prompt" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>ℹ️ 8. On-Device Reference</h3>
      <p>A built-in tactical manual. If you need a quick refresher on module definitions or the color-coded intensity spectrum (e.g., matching 'Dark Indigo' to 65% Intensity), the reference guide is instantly accessible without interrupting your workflow.</p>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/IMG_7958.jpg" alt="On-Device Manual" width="100%">
    </td>
  </tr>
</table>

<hr>

<table width="100%">
  <tr>
    <td width="65%" valign="top">
      <h3>👾 System Architects</h3>
      <ul>
        <li><b>Maryam Odat</b> (<code>modatt</code>)</li>
        <li><b>Abdullah Alawad</b> (<code>Abdullah-Alawad</code>)</li>
      </ul>
    </td>
    <td width="35%" align="center" valign="middle">
      <img src="https://github.com/joverse962/Images/blob/main/LiteTUI/names.jpg" alt="Credits Screen" width="100%">
    </td>
  </tr>
</table>

## Authors

- Maryam Odat <mariam.odat97@gmail.com>
- Abdullah Al-Awad <abdullahalawad123321@gmail.com>

## Contributors

<!-- Add future contributors here -->

## Contributing

Contributions are welcome! If you have ideas for new features, bug fixes, or improvements, please open an issue or submit a pull request.

## License

This project is licensed under the Apache License, Version 2.0. See the `LICENSE` file for more details.
