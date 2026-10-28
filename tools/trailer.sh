#!/bin/bash
# Cuts the World of Farmcraft trailer from the gameplay recording.
set -e
T=/path/to/file
V="/path/to/file"
LOGO=/path/to/file
FONT=georgiab.ttf
mkdir -p $T/seg
cd $T

# Shared look: 1920x1080, 30 fps, stereo 48k.
enc="-c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p -r 30 -c:a aac -b:a 192k -ar 48000 -ac 2"

# A gameplay clip: start, length, caption.
clip() {
  local n=$1 ss=$2 len=$3 text=$4
  local fo=$(awk "BEGIN{print $len-0.3}")
  cat > seg/$n.txt <<EOF
[0:v]scale=1920:1080,fade=t=in:st=0:d=0.25,fade=t=out:st=$fo:d=0.3,drawtext=fontfile='$FONT':text='$text':x=(w-text_w)/2:y=h-170:fontsize=58:fontcolor=0xFFD34D:borderw=4:bordercolor=0x2B1D10:shadowx=3:shadowy=3:alpha='if(lt(t,0.3),t/0.3,if(gt(t,$len-0.4),($len-t)/0.4,1))'[v];
[0:a]volume=0.8,afade=t=in:st=0:d=0.25,afade=t=out:st=$fo:d=0.3[a]
EOF
  ffmpeg -v error -y -ss $ss -i "$V" -t $len -/filter_complex seg/$n.txt -map "[v]" -map "[a]" $enc seg/$n.mp4
}

# A logo card: the crest slowly growing, with a line under it.
card() {
  local n=$1 len=$2 text=$3
  local fo=$(awk "BEGIN{print $len-0.6}")
  cat > seg/$n.txt <<EOF
[0:v]scale=1920:-2,zoompan=z='1+0.04*on/($len*30)':d=1:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s=1920x1076:fps=30,pad=1920:1080:0:2:black,fade=t=in:st=0:d=0.6,fade=t=out:st=$fo:d=0.6,drawtext=fontfile='$FONT':text='$text':x=(w-text_w)/2:y=h-150:fontsize=46:fontcolor=0xF3E6C4:borderw=3:bordercolor=black[v]
EOF
  ffmpeg -v error -y -loop 1 -framerate 30 -t $len -i "$LOGO" -f lavfi -t $len -i anullsrc=r=48000:cl=stereo -/filter_complex seg/$n.txt -map "[v]" -map 1:a $enc -t $len seg/$n.mp4
}

card  00 3.5 "A cozy farming life in Elwynn Forest"
clip  01   5.5 2.5 "Start with a shovel and a pouch of seeds"
clip  02  18.0 4.0 "Plant"
clip  03  50.5 2.5 "Water"
clip  04 108.0 4.0 "Saddle up"
clip  05 129.0 2.5 "Ride to Goldshire"
clip  06 145.5 2.5 "Sell at the Lion’s Pride Inn"
clip  07 226.0 2.5 "Day 10"
clip  08 231.0 2.5 "Day 25"
clip  09 236.0 2.5 "Day 50"
clip  10 241.0 2.8 "Day 100"
clip  11 249.0 3.5 "Chop timber"
clip  12 256.3 2.6 "Raise a herd"
clip  13 279.0 3.5 "Mine Fargodeep"
card  14 3.5 "Coming soon"

ls seg/*.mp4 | sort | sed 's/^/file /' > list.txt
ffmpeg -v error -y -f concat -safe 0 -i list.txt -c copy farmcraft-trailer.mp4
ffprobe -v error -show_entries format=duration -of csv=p=0 farmcraft-trailer.mp4
