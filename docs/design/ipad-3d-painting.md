# 3D painting on the iPad

The iPad app had no 3D painting: its gallery turned every model file away. The core already did the work for the other two apps (the model's drawing, strokes carried onto it, and its touches: a finger turns it, two move and zoom, the Pencil paints), so the iPad needed only its own interface:

- **Gallery** (`GalleryModel.swift`): Import opens OBJ, USD, a zip holding a model with its files, and a ProCraft model artwork.
- **Canvas model** (`CanvasModel.swift`): whether a model is open, its texture sets, Show 2D texture, Paint through mesh. A model is lit by the `mountain` environment (the Android app's panoramas are now in the iPad app too).
- **Actions**: on a model, a 3D tab in Video's place (Show 2D texture, Paint through mesh), Share Model (ProCraft, USDZ, OBJ), Share Image, Share Textures, and the Add and Canvas tabs cut down to what applies.
- **Layers**: the texture sets above the layers, the one painted on lit; a tap paints on another. No background row.

- **Gesture controls**: General's switch for painting a model with a finger shows now (off, a finger turns the model and the Pencil paints).

Tried in the simulator with a sample model: it opens lit and textured, a finger turns it, and with the finger-painting switch on, strokes land on it. The Pencil can't be simulated; it goes through the same path in the core.

Not on the iPad yet, all there on Android to port from: the Lighting Studio, the meshes list (hide, lock, pick), roughness and metalness tiles, placing a picture on the model's surface (Insert and Paste are dimmed unless the 2D texture shows), turntable exports, the model in the Reference window, AR.
