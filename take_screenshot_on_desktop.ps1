Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$Screen = [System.Windows.Forms.Screen]::PrimaryScreen
$Bitmap = New-Object System.Drawing.Bitmap $Screen.Bounds.Width, $Screen.Bounds.Height
$Graphic = [System.Drawing.Graphics]::FromImage($Bitmap)
$Graphic.CopyFromScreen($Screen.Bounds.X, $Screen.Bounds.Y, 0, 0, $Bitmap.Size)
$Bitmap.Save('E:\code\sandbox\sandbox-plus\screen-custom.png', [System.Drawing.Imaging.ImageFormat]::Png)
$Graphic.Dispose()
$Bitmap.Dispose()
