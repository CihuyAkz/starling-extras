# Android Java mod storage

The Starling Java bridge reads Fabric JARs from public shared storage, under the active game profile inside `StarlingExtras`.

Default profile:

```text
/storage/emulated/0/StarlingExtras/
└── mods-java/
    ├── mod-menu.jar
    └── other-fabric-mod.jar
```

When a non-default launcher profile is selected, its folder is inside `StarlingExtras/profiles/profile-N/`, and `mods-java/` belongs inside that profile.

The Java bridge does not load JARs from the app-private `getFilesDir()` or `HORIZON_PRIVATE` directory. Android's All files access permission must be enabled for Starling. Until it is enabled, Java JAR imports are rejected and the bridge does not scan a private fallback directory. Private app storage remains available for the Java runtime, preparation cache, temporary bridge files, and import inbox.
On Android, `LauncherActivity` requires the app's public shared-storage permission before it starts the game. Android 11 and later use the app-specific All files access setting; older versions request legacy storage permission. The app does not start the game while this permission is denied, so the normal launch path cannot silently select the private game directory.

