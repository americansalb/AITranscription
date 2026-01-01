# Scribe Desktop App - Troubleshooting

## macOS: "App is Damaged" or "Cannot be Opened"

This error occurs because the app is not signed with an Apple Developer certificate. macOS Gatekeeper blocks unsigned applications.

### Quick Fix (Recommended for Testing)

Open Terminal and run:

```bash
xattr -cr /path/to/Scribe.app
```

Replace `/path/to/Scribe.app` with the actual path. If you copied it to Applications:

```bash
xattr -cr /Applications/Scribe.app
```

Then try opening the app again.

### Alternative: Right-Click to Open

1. Right-click (or Control-click) on Scribe.app
2. Select "Open" from the context menu
3. Click "Open" in the dialog that appears

This bypasses Gatekeeper for this specific app.

### Why This Happens

Since macOS Catalina (10.15), Apple requires all distributed apps to be:
1. Signed with an Apple Developer ID certificate
2. Notarized by Apple

Without these, Gatekeeper shows security warnings or "damaged" errors.

---

## Windows: App Doesn't Start / Nothing Happens

### Check for Error Log

The app now logs startup errors. Check for:
```
C:\Users\<YourUsername>\scribe-error.log
```

This file will contain any error messages if the app failed to start.

### Windows SmartScreen Blocking

Windows SmartScreen may silently block unsigned applications:

1. When you first run the installer or app, look for a SmartScreen popup
2. If you see "Windows protected your PC":
   - Click "More info"
   - Click "Run anyway"

### WebView2 Runtime Required

Scribe requires Microsoft WebView2 Runtime. The installer should install it automatically, but if it fails:

1. Download WebView2 Runtime from: https://developer.microsoft.com/en-us/microsoft-edge/webview2/
2. Install the "Evergreen Bootstrapper" or "Evergreen Standalone Installer"
3. Restart your computer
4. Try running Scribe again

### Visual C++ Redistributable

Some Windows systems need the Visual C++ Redistributable:

1. Download from: https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist
2. Install the x64 version
3. Restart and try again

### Antivirus/Security Software

Some antivirus programs block unsigned applications:

1. Check your antivirus quarantine/blocked list
2. Add an exception for Scribe if needed
3. Temporarily disable real-time protection to test

---

## Linux: App Doesn't Start

### AppImage Permissions

```bash
chmod +x Scribe*.AppImage
./Scribe*.AppImage
```

### Missing Dependencies

For the .deb package, you may need:

```bash
sudo apt-get install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1
```

---

## macOS: Global Hotkey Not Working

If the `Ctrl+Space` push-to-talk hotkey doesn't work:

### 1. Check Console for Errors

Open the app and check if the hotkey registers in the console (View > Developer > Developer Tools).

### 2. Alternative: Use Click-to-Record

If the hotkey still doesn't work, you can click the record button in the app window instead.

---

## macOS: Microphone Permission Error

If you see an error like `undefined is not an object (evaluating 'navigator.mediaDevices.getUserMedia')` or "Microphone access not available":

### Grant Microphone Permission

1. Open **System Settings** (or System Preferences on older macOS)
2. Go to **Privacy & Security** > **Microphone**
3. Find **Scribe** in the list and enable the toggle
4. If Scribe isn't listed, try clicking the record button once to trigger the permission prompt
5. Restart Scribe after granting permission

### Alternative: Reset Permissions

If permission was previously denied:

```bash
tccutil reset Microphone com.scribe.app
```

Then relaunch Scribe and grant permission when prompted.

---

## Backend Connection Issues

If the app starts but shows "Cannot connect to backend":

1. Ensure the backend server is running
2. Check that `VITE_API_URL` is set correctly
3. The default backend URL is `http://localhost:8000`

---

## Reporting Issues

If these solutions don't work:

1. Check the error log (Windows: `~/scribe-error.log`)
2. Open an issue with:
   - Your operating system version
   - Error message or log contents
   - Steps to reproduce
