import gatekeeper_py

r=gatekeeper_py.Reader("pn532_uart:/dev/ttyACM0", gatekeeper_py.RealmType.MemberProjects)

while True:
  tag = r.poll_for_tag()
  if tag:
    print("OMG!!! TAG!!!")
    print("Tag: " + str(tag.get_tag_type()))
    try:
      aid = tag.get_association()
    except Exception as e:
      print("oops: " + str(e))
      continue
    print("Association ID: " + aid)
    user = r.fetch_user(aid)
    user = user["user"]
    print("Username: " + user["uid"])
    print("Hello " + user["cn"])
