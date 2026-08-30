# Kiln Button Behavior Requirements

This document records the finalized behavior decisions for the approved Option 1 layout. UI implementation comes first. Interaction logic follows only after the layout receives final approval.

## Queue Selection

1. Adding files automatically selects the first queue row.
2. Clicking a row makes it the active row.
3. Up Arrow moves the active selection to the previous row.
4. Down Arrow moves the active selection to the next row.
5. Mouse wheel scrolls the queue.
6. Shift+Click toggles only the clicked row in the multi-selection.
7. Shift+Arrow extends a continuous row range.
8. Command+A selects every row on macOS.
9. Control+A selects every row on Windows.
10. Command+Click has no special behavior on macOS.
11. Control+Click has no special behavior on Windows.
12. Command or Control alone performs no selection action.
13. The last clicked row becomes the active row during multi-selection.
14. Current Dimension shows the active row's original width × height.
15. New Dimension shows only the active row's target dimension.
16. Entering a New Dimension applies the value to every selected row.
17. Moving to another active row shows that row's target dimension.
18. Remove Selected appears beside Clear All when two or more rows are selected.
19. Remove Selected shows the selected row count.
20. Delete removes selected rows on Windows.
21. Delete/Backspace removes selected rows on macOS.
22. Delete/Backspace inside an input, select, or textarea edits that control only.
23. Removing the active row selects the next available row.
24. Removing the final row selects the previous available row.
25. Removing every row resets Format to Select A Type.
26. Removing every row resets Current Dimension to —.
27. Removing every row clears New Dimension.
28. Clear All always clears the entire queue regardless of row selection.

## Clear

### Empty Queue

1. Clear is disabled.

### Files Queued Before Conversion

1. Clear is enabled for any queue size.
2. Clicking Clear removes every queue record.
3. Original files stay untouched.
4. Queue becomes empty.
5. Clear becomes disabled.
6. Convert becomes disabled.
7. Format returns to Select A Type.
8. Current Dimension returns to —.
9. New Dimension becomes empty.

### Conversion Running

1. Clear is disabled.
2. Convert changes to Cancel.
3. Cancel controls the in-flight conversion.

### Conversion Finished Before Save

1. Successful rows carry an internal Converted — Unsaved state.
2. Clear is enabled.
3. Clicking Clear opens Discard Unsaved Converted Files?
4. The popup contains Cancel plus Discard Files.
5. Cancel closes the popup.
6. Cancel keeps queue records.
7. Cancel keeps temporary converted files.
8. Discard Files deletes temporary converted files.
9. Discard Files removes every queue record.
10. Original source files stay untouched.
11. Previously saved destination files stay untouched.
12. Queue becomes empty.
13. Clear becomes disabled.

### Save Finished

1. Clear is enabled while rows remain.
2. Clicking Clear removes every remaining queue record without a popup.
3. Saved rows disappear from the queue.
4. Failed rows disappear from the queue.
5. Cancelled rows disappear from the queue.
6. Saved destination files stay untouched.
7. Original source files stay untouched.
8. Queue becomes empty.
9. Clear becomes disabled.

## Choose Files

1. Choose Files is enabled before conversion.
2. Clicking Choose Files opens the native multi-file picker.
3. The picker supports HEIC, AVIF, WebP, JPEG, PNG, TIFF.
4. Cancelling the picker leaves the queue unchanged.
5. Supported selections enter the queue.
6. Duplicate files are ignored.
7. Unsupported files are rejected.
8. Corrupt images show Failed.
9. Valid images show file name, size, Current Dimension, Ready.
10. Choose Files is disabled during conversion.
11. Choose Files is disabled while converted results await saving.
12. Choose Files becomes enabled after Save or Clear.
13. One selected image plus multiple selected images use the same queue flow.
14. The first newly added row becomes active when no row is already active.
15. The picker disables Choose Files until the picker closes.
16. Repeated clicks cannot open multiple pickers.
17. Supported extension aliases include HEIF, JPG, JPEG, JFIF, TIF, plus TIFF.
18. The queue accepts at most 20,000 files from one collected selection.
19. Picker failures show Could Not Open File Picker.
20. File-reading failures show Could Not Read Selected Files.

## Remove File

1. Every queue row has its own Remove button.
2. Clicking Remove before conversion deletes only that queue record.
3. Original files stay untouched.
4. Remove is disabled during conversion.
5. Clicking Remove after successful conversion opens Discard This Unsaved Converted File?
6. Cancel keeps the queue row.
7. Discard File deletes only that temporary result.
8. Discard File removes only that queue row.
9. Clicking Remove after Save removes the queue row without a popup.
10. Saved destination files stay untouched.
11. Failed rows disappear without a popup.
12. Cancelled rows disappear without a popup.
13. Removing the selected row updates Current Dimension to the next active row.
14. Removing the last row disables Clear plus Convert.
15. Removing multiple queued, failed, cancelled, or saved rows requires no popup.
16. Removing multiple rows containing any unsaved converted result opens Discard Selected Unsaved Converted Files?
17. The multi-row popup contains Cancel plus Discard Files.
18. Cancel keeps every selected row plus every temporary result.
19. Discard Files removes selected rows plus their temporary results only.
20. Remove Selected is disabled during conversion.

## Drop Images Here

1. Dropping one image uses the same queue flow as Choose Files.
2. Dropping multiple images uses the same queue flow as Choose Files.
3. Dropping a folder collects supported images up to eight folder levels deep.
4. An empty folder shows No Supported Images Found.
5. Folder content is sorted before it enters the queue.
6. Supported extension matching is case-insensitive.
7. Duplicate paths plus symlink aliases are ignored.
8. Unsupported files are ignored with a visible notice.
9. Corrupt supported-extension files show Failed.
10. One corrupt file does not prevent valid files from showing Ready.
11. A drop accepts at most 20,000 supported files.
12. A larger drop shows 20,000 File Limit Reached.
13. Folders deeper than eight levels show Folder Depth Limit Reached.
14. Unreadable folders show Some Folders Could Not Be Read.
15. A drag hover highlights the drop area only while intake is enabled.
16. Leaving the drop area removes the hover state.
17. Dropping removes the hover state immediately.
18. Drop is disabled while conversion runs.
19. Drop is disabled while converted results await saving.
20. Drop is disabled while Choose Files or another drop is processing.
21. Repeated concurrent drops cannot start overlapping intake operations.
22. Drop becomes enabled after Save or Clear.

## Convert / Cancel

1. Convert is disabled when the queue is empty.
2. Convert is disabled while Format shows Select A Type.
3. Convert becomes enabled after a format is selected.
4. Convert remains disabled when every queue row is corrupt, converted, saved, or otherwise not retryable.
5. Clicking Convert starts every Ready, Failed-retryable, or Cancelled queue row.
6. Converted plus Saved rows never convert again automatically.
7. Unsaved converted results block another conversion until Save, Remove, or Clear resolves them.
8. Rows without a New Dimension keep their original dimensions.
9. Rows with a New Dimension use their assigned exact width × height.
10. Different rows may use different target dimensions in one batch.
7. New Dimension accepts eight digits.
8. The field automatically formats eight digits as XXXX * YYYY.
9. The star has one space on each side.
10. Each dimension must be at least 1024 px.
11. Each dimension must be at most 7680 px.
12. Partial input does not trigger a popup.
13. Completed invalid input opens Min Is 1024 px · Max Is 7680 px.
14. The popup contains one Close button.
15. Close returns focus to New Dimension.
16. The invalid value stays available for correction.
17. Convert never receives an incomplete, malformed, or out-of-range dimension.
18. Original files stay untouched.
19. Converted results write to temporary storage.
20. Every started row changes from Ready, Failed, or Cancelled to Converting.
21. Successful rows show Converted.
22. Unsuccessful rows show Failed.
23. One failed file does not stop other files.
24. Convert changes to Cancel during processing.
25. One Cancel click changes Cancel to Cancelling plus disables repeated cancellation requests.
26. Clear, Choose Files, Remove Selected, Remove, Format, Dimensions, Quality, Metadata, Timestamp become disabled during processing.
27. Clicking Cancel stops files that have not started.
28. Files already processing finish safely.
29. Cancelled rows show Cancelled.
30. Conversion completion restores Convert.
31. Save becomes enabled when at least one file converts successfully.
32. Convert becomes retryable after an all-failed or all-cancelled batch.
33. A mixed batch reports Converted, Failed, plus Cancelled per row without stopping successful files.
34. Conversion-start failure restores every pre-conversion row state.
35. Original source files remain byte-for-byte untouched.

## Save

1. Save stays visible in a disabled state before conversion.
2. Save becomes enabled after at least one successful conversion.
3. Clicking Save opens destination choices.
4. The editable Target Folder initially shows the first eligible image's source folder.
5. Choose Folder replaces the Target Folder with one selected folder.
6. Every eligible image is saved into that Target Folder.
8. Cancelling the folder picker keeps the current destination choice.
9. A single image shows an editable File Name plus its fixed converted extension.
10. A timestamped conversion keeps its timestamp in the default File Name.
11. Unsafe filename characters cannot create subfolders or invalid Windows filenames.
12. An empty filename safely falls back to the converted filename.
13. A batch preserves every converted filename.
14. Exact path collisions open File Already Exists or Files Already Exist.
15. The collision popup contains Cancel, Overwrite, plus Keep Both.
16. Cancel writes nothing plus returns to the Save dialog.
17. Keep Both preserves every existing destination file.
18. Keep Both generates the first available numbered filename.
19. Overwrite replaces only after the complete new file is safely copied.
20. A failed replacement leaves the prior destination recoverable.
21. Original source files stay untouched unless the user explicitly confirms an exact source-path overwrite.
22. Saving disables Convert, Clear All, Choose Files, Remove Selected, Remove, plus repeated Save clicks until completion.
23. One failed save does not stop successful files.
24. Successful rows show Saved.
25. Failed save rows show Failed plus retain their temporary result for retry.
26. Save remains enabled for failed save rows plus can retry them without reconversion.
27. Saved, Failed, plus Cancelled rows remain available for Clear.
28. Clear or Remove after Save never deletes destination files.

## Compression Quality

1. The control is named Compression Quality.
2. The minimum value is 40.
3. The maximum value is 100.
4. The default value is 80.
5. The displayed number updates while the slider moves.
6. The control is visible for JPEG, WebP, AVIF, plus HEIC.
7. The control is hidden for PNG plus TIFF.

## Keep Metadata

1. Keep Metadata is unchecked by default.
2. Unchecked removes EXIF, GPS, camera, timestamp, XMP, IPTC, C2PA, plus AI-generation tags from converted files.
3. Unchecked retains the color profile so converted colors remain accurate.
4. Checked preserves all metadata supported by the selected output format.
5. The setting affects converted copies only.
6. Original source files stay untouched.
7. The setting does not change pixel dimensions.
8. The setting does not change Compression Quality.

## Add Timestamp To Filename

1. Add Timestamp To Filename is unchecked by default.
2. Unchecked keeps the converted filename without a timestamp.
3. Checked appends a local timestamp before the file extension.
4. The timestamp format is YYYYMMDD-HHMMSS.
5. Example: photo-20260824-131205.jpg.
6. Every file in one batch receives the same timestamp.
7. The timestamp contains no slash, backslash, or colon.
8. Original source files stay untouched.
